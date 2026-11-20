//! Code discovery: which bytes of the code section are instructions, where
//! functions start, and what remains unexplained.
//!
//! The executable ships its base relocations, so every absolute address stored
//! anywhere in the image is known. That turns the usual guesswork of static
//! disassembly into bookkeeping:
//!
//! * a relocated immediate that points into the code section is a code pointer;
//! * a relocated memory displacement that points there is data kept in the code
//!   section (switch index tables, constants);
//! * a jump table is a run of relocated words, read until the relocations stop.
//!
//! Discovery is a recursive traversal with two confidence levels. Code reached
//! by direct control flow is decoded first; code only known through a stored
//! pointer is decoded afterwards, so that a wrong guess can never overwrite a
//! certain result. Anything inconsistent is recorded, never silently dropped.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction, Mnemonic, OpKind, Register};
use serde::Serialize;

use crate::pe::{Image, Section};

const UNKNOWN: u8 = 0;
const INSN_START: u8 = 1;
const INSN_TAIL: u8 = 2;
const JUMP_TABLE: u8 = 3;
const PADDING: u8 = 4;
const DATA: u8 = 5;

/// How an address came to be considered a function entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// The program entry point.
    Entry,
    /// Target of at least one direct `call`.
    Called,
    /// Only referenced through a stored pointer (callback, vtable, handler).
    AddressTaken,
    /// Unreferenced code found in a gap between known functions.
    Orphan,
}

#[derive(Debug, Clone, Serialize)]
pub struct Function {
    pub entry: u32,
    pub kind: EntryKind,
    pub instructions: u32,
    pub bytes: u32,
    /// Lowest and one-past-highest address of its instructions.
    pub start: u32,
    pub end: u32,
    /// Direct jumps or fall-throughs into another function's entry.
    pub tail_transfers: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Bytes that do not decode to a valid instruction.
    Invalid,
    /// An instruction would start inside, or run into, an already known one.
    Overlap,
    /// Control flow leaves the code section.
    OutOfSection,
    /// Far branches and software interrupts: not expected in this program.
    Unexpected,
}

#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub at: u32,
    pub kind: ConflictKind,
    /// True if the path that led here came from a stored pointer or a guess.
    pub speculative: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Gap {
    pub start: u32,
    pub len: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportUse {
    pub dll: String,
    pub name: String,
    pub call_sites: u32,
    pub jump_sites: u32,
    /// References that neither call nor jump (address stored or loaded).
    pub other_refs: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ByteCounts {
    pub code: u32,
    pub jump_tables: u32,
    pub padding: u32,
    pub data: u32,
    pub unknown: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub code_start: u32,
    pub code_len: u32,
    pub bytes: ByteCounts,
    pub instructions: u32,
    pub functions: Vec<Function>,
    /// Instructions that belong to more than one function.
    pub shared_instructions: u32,
    /// Stored code pointers that land inside a function (handlers, labels).
    pub address_taken_labels: Vec<u32>,
    pub jump_tables: u32,
    pub jump_table_entries: u32,
    pub direct_calls: u32,
    pub indirect_calls: u32,
    /// Indirect jumps that are neither a recognised jump table nor an import.
    pub unresolved_indirect_jumps: Vec<u32>,
    pub conflicts: Vec<Conflict>,
    pub unknown_gaps: Vec<Gap>,
    pub imports: Vec<ImportUse>,
    pub mnemonics: BTreeMap<String, u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confidence {
    /// Reached by direct control flow from the entry point.
    Certain,
    /// Reached only through a stored pointer or a gap scan.
    Speculative,
}

struct Discovery<'a> {
    image: &'a Image,
    text: &'a Section,
    kind: Vec<u8>,
    /// Per byte: the instruction starting here is reached by fall-through.
    fallen_into: Vec<bool>,
    certain: VecDeque<u32>,
    speculative: VecDeque<u32>,
    call_targets: BTreeSet<u32>,
    address_taken: BTreeSet<u32>,
    orphans: BTreeSet<u32>,
    /// Targets of conditional branches and jump tables.
    local_targets: HashSet<u32>,
    /// Code-section addresses used as data through a memory operand.
    data_refs: BTreeSet<u32>,
    tables: HashMap<u32, Vec<u32>>,
    unresolved_indirect_jumps: Vec<u32>,
    conflicts: Vec<Conflict>,
    import_uses: HashMap<u32, [u32; 3]>,
    direct_calls: u32,
    indirect_calls: u32,
}

/// What follows an instruction, as far as static analysis can tell.
struct Flow {
    falls_through: bool,
    /// Direct or jump-table targets.
    targets: Vec<u32>,
}

impl<'a> Discovery<'a> {
    fn in_text(&self, va: u32) -> bool {
        va >= self.text.va && ((va - self.text.va) as usize) < self.text.data.len()
    }

    fn index(&self, va: u32) -> usize {
        (va - self.text.va) as usize
    }

    fn decode(&self, va: u32) -> Option<(Instruction, Decoder<'a>)> {
        let bytes = &self.text.data[self.index(va)..];
        let mut decoder = Decoder::with_ip(32, bytes, va as u64, DecoderOptions::NONE);
        let instruction = decoder.decode();
        (!instruction.is_invalid()).then_some((instruction, decoder))
    }

    fn conflict(&mut self, at: u32, kind: ConflictKind, confidence: Confidence) {
        self.conflicts.push(Conflict {
            at,
            kind,
            speculative: confidence == Confidence::Speculative,
        });
    }

    /// Reads the jump table used by `jmp [index*4 + table]`.
    ///
    /// Leading slots without a relocation are skipped (tables indexed from 1).
    /// The table ends where relocations stop, where an entry leaves the code
    /// section, or where known code begins. Two tables laid out back to back
    /// are read as one: an over-approximation of the successors, never a miss.
    fn read_jump_table(&mut self, table: u32) -> Vec<u32> {
        let mut targets = Vec::new();
        let mut slot = table;
        // Hand-written runtime code indexes some tables from 1 or higher, so
        // the nominal base lies before the first real entry.
        for _ in 0..8 {
            if self.image.is_reloc(slot) {
                break;
            }
            slot += 4;
        }
        while self.in_text(slot)
            && self.in_text(slot + 3)
            && self.image.is_reloc(slot)
            && self.kind[self.index(slot)..self.index(slot) + 4]
                .iter()
                .all(|&k| k == UNKNOWN || k == JUMP_TABLE || k == DATA)
        {
            let Some(target) = self.image.read_u32(slot).filter(|&t| self.in_text(t)) else {
                break;
            };
            targets.push(target);
            let at = self.index(slot);
            self.kind[at..at + 4].fill(JUMP_TABLE);
            slot += 4;
        }
        targets
    }

    /// Classifies the relocated operands of an instruction and its control flow.
    fn examine(&mut self, instruction: &Instruction, decoder: &Decoder<'_>, confidence: Confidence) -> Flow {
        let ip = instruction.ip() as u32;
        let next = instruction.next_ip() as u32;
        let offsets = decoder.get_constant_offsets(instruction);
        let displacement_at = (offsets.has_displacement() && offsets.displacement_size() == 4)
            .then(|| ip + offsets.displacement_offset() as u32);
        let flow_control = instruction.flow_control();

        let mut table_base = None;
        let mut import_slot = None;
        if instruction.op_count() > 0 && instruction.op0_kind() == OpKind::Memory
            || instruction.op_count() > 1 && instruction.op1_kind() == OpKind::Memory
        {
            let absolute = instruction.memory_base() == Register::None;
            let displacement = instruction.memory_displacement32();
            if absolute
                && instruction.memory_index() == Register::None
                && self.image.import_at(displacement).is_some()
            {
                import_slot = Some(displacement);
            }
            if flow_control == FlowControl::IndirectBranch
                && absolute
                && instruction.memory_index() != Register::None
                && instruction.memory_index_scale() == 4
                && displacement_at.is_some_and(|at| self.image.is_reloc(at))
                && self.in_text(displacement)
            {
                table_base = Some(displacement);
            }
        }

        for &reloc in self.image.relocs_in(ip, next) {
            let Some(value) = self.image.read_u32(reloc).filter(|&v| self.in_text(v)) else {
                continue;
            };
            if Some(reloc) == displacement_at {
                if table_base.is_none() {
                    self.data_refs.insert(value);
                }
            } else if self.address_taken.insert(value) {
                self.speculative.push_back(value);
            }
        }

        if let Some(slot) = import_slot {
            let column = match flow_control {
                FlowControl::IndirectCall => 0,
                FlowControl::IndirectBranch => 1,
                _ => 2,
            };
            self.import_uses.entry(slot).or_default()[column] += 1;
        }

        let mut flow = Flow {
            falls_through: false,
            targets: Vec::new(),
        };
        let direct_target = |this: &mut Self| -> Option<u32> {
            match instruction.op0_kind() {
                OpKind::NearBranch16 | OpKind::NearBranch32 => {
                    let target = instruction.near_branch_target() as u32;
                    if this.in_text(target) {
                        Some(target)
                    } else {
                        this.conflict(ip, ConflictKind::OutOfSection, confidence);
                        None
                    }
                }
                _ => {
                    this.conflict(ip, ConflictKind::Unexpected, confidence);
                    None
                }
            }
        };
        match flow_control {
            FlowControl::Next => flow.falls_through = true,
            FlowControl::ConditionalBranch => {
                flow.falls_through = true;
                if let Some(target) = direct_target(self) {
                    self.local_targets.insert(target);
                    flow.targets.push(target);
                }
            }
            FlowControl::UnconditionalBranch => flow.targets.extend(direct_target(self)),
            FlowControl::Call => {
                flow.falls_through = true;
                self.direct_calls += 1;
                if let Some(target) = direct_target(self) {
                    self.call_targets.insert(target);
                    self.certainty_queue(confidence).push_back(target);
                }
            }
            FlowControl::IndirectCall => {
                flow.falls_through = true;
                self.indirect_calls += 1;
            }
            FlowControl::IndirectBranch => {
                if let Some(table) = table_base {
                    let targets = self.read_jump_table(table);
                    self.local_targets.extend(&targets);
                    flow.targets.extend(&targets);
                    self.tables.insert(ip, targets);
                } else if import_slot.is_none() {
                    self.unresolved_indirect_jumps.push(ip);
                }
            }
            FlowControl::Return => {}
            FlowControl::Interrupt => {
                // `int3` is padding or a debug trap; other interrupts are not
                // something a Win32 program of this kind executes.
                if instruction.mnemonic() != Mnemonic::Int3 {
                    self.conflict(ip, ConflictKind::Unexpected, confidence);
                }
            }
            FlowControl::Exception | FlowControl::XbeginXabortXend => {
                self.conflict(ip, ConflictKind::Unexpected, confidence);
            }
        }
        flow
    }

    fn certainty_queue(&mut self, confidence: Confidence) -> &mut VecDeque<u32> {
        match confidence {
            Confidence::Certain => &mut self.certain,
            Confidence::Speculative => &mut self.speculative,
        }
    }

    /// Decodes straight-line code from `start` until it ends or meets known code.
    fn trace(&mut self, start: u32, confidence: Confidence) {
        let mut ip = start;
        loop {
            if !self.in_text(ip) {
                self.conflict(ip, ConflictKind::OutOfSection, confidence);
                return;
            }
            match self.kind[self.index(ip)] {
                INSN_START => return,
                UNKNOWN => {}
                _ => {
                    self.conflict(ip, ConflictKind::Overlap, confidence);
                    return;
                }
            }
            let Some((instruction, decoder)) = self.decode(ip) else {
                self.conflict(ip, ConflictKind::Invalid, confidence);
                return;
            };
            if is_unexpected(&instruction) {
                self.conflict(ip, ConflictKind::Unexpected, confidence);
                return;
            }
            let at = self.index(ip);
            let len = instruction.len();
            if self.kind[at..at + len].iter().any(|&k| k != UNKNOWN) {
                self.conflict(ip, ConflictKind::Overlap, confidence);
                return;
            }
            self.kind[at] = INSN_START;
            self.kind[at + 1..at + len].fill(INSN_TAIL);

            let flow = self.examine(&instruction, &decoder, confidence);
            for &target in &flow.targets {
                self.certainty_queue(confidence).push_back(target);
            }
            if !flow.falls_through {
                return;
            }
            ip = instruction.next_ip() as u32;
            if self.in_text(ip) {
                let next = self.index(ip);
                self.fallen_into[next] = true;
            }
        }
    }

    fn run(&mut self) {
        loop {
            if let Some(ip) = self.certain.pop_front() {
                self.trace(ip, Confidence::Certain);
            } else if let Some(ip) = self.speculative.pop_front() {
                self.trace(ip, Confidence::Speculative);
            } else {
                return;
            }
        }
    }

    /// Maximal runs of bytes with the given classification.
    fn runs(&self, wanted: u8) -> Vec<Gap> {
        let mut runs = Vec::new();
        let mut at = 0;
        while at < self.kind.len() {
            if self.kind[at] != wanted {
                at += 1;
                continue;
            }
            let start = at;
            while at < self.kind.len() && self.kind[at] == wanted {
                at += 1;
            }
            runs.push(Gap {
                start: self.text.va + start as u32,
                len: (at - start) as u32,
            });
        }
        runs
    }

    /// Marks alignment filler and referenced data inside unexplained gaps.
    fn classify_gaps(&mut self) {
        let is_filler = |b: u8| b == 0x90 || b == 0xCC;
        for gap in self.runs(UNKNOWN) {
            let (mut lo, mut hi) = (self.index(gap.start), self.index(gap.start) + gap.len as usize);
            while lo < hi && is_filler(self.text.data[lo]) {
                self.kind[lo] = PADDING;
                lo += 1;
            }
            while hi > lo && is_filler(self.text.data[hi - 1]) {
                hi -= 1;
                self.kind[hi] = PADDING;
            }
            // From the first data reference the gap is data, up to filler that
            // ends on a 16-byte boundary: what follows may be a function.
            let first = self.text.va + lo as u32;
            let last = self.text.va + hi as u32;
            if let Some(&data) = self.data_refs.range(first..last).next() {
                let from = self.index(data);
                let boundary = |at: usize| (self.text.va as usize + at).is_multiple_of(16);
                let end = (from + 1..hi)
                    .find(|&at| boundary(at) && is_filler(self.text.data[at - 1]))
                    .unwrap_or(hi);
                let mut data_end = end;
                while data_end > from + 1 && end < hi && is_filler(self.text.data[data_end - 1]) {
                    data_end -= 1;
                }
                self.kind[from..data_end].fill(DATA);
                self.kind[data_end..end].fill(PADDING);
            }
        }
    }

    /// Checks that a gap holds plausible, self-contained code before trusting it.
    fn plausible_code(&self, gap: &Gap) -> bool {
        let end = gap.start + gap.len;
        let mut ip = gap.start;
        // Compiled functions are aligned to 16 bytes. Hand-written runtime
        // routines are not, but they directly follow other code. Anything
        // else that merely happens to decode is data.
        let follows_code = gap.start > self.text.va
            && matches!(self.kind[self.index(gap.start) - 1], INSN_START | INSN_TAIL);
        if !gap.start.is_multiple_of(16) && !follows_code {
            return false;
        }
        let lands_on_code = |target: u32| {
            self.in_text(target) && matches!(self.kind[self.index(target)], INSN_START | UNKNOWN)
        };
        loop {
            if ip == end {
                // Falls into whatever follows the gap: fine only if that is code.
                return self.in_text(ip) && self.kind[self.index(ip)] == INSN_START;
            }
            let Some((instruction, _)) = self.decode(ip) else {
                return false;
            };
            if instruction.next_ip() as u32 > end || is_unexpected(&instruction) {
                return false;
            }
            match instruction.flow_control() {
                FlowControl::Next | FlowControl::IndirectCall => {}
                FlowControl::Call | FlowControl::ConditionalBranch => {
                    if !matches!(
                        instruction.op0_kind(),
                        OpKind::NearBranch32 | OpKind::NearBranch16
                    ) || !lands_on_code(instruction.near_branch_target() as u32)
                    {
                        return false;
                    }
                }
                FlowControl::UnconditionalBranch => {
                    return matches!(
                        instruction.op0_kind(),
                        OpKind::NearBranch32 | OpKind::NearBranch16
                    ) && lands_on_code(instruction.near_branch_target() as u32);
                }
                FlowControl::Return | FlowControl::IndirectBranch => return true,
                FlowControl::Interrupt | FlowControl::Exception | FlowControl::XbeginXabortXend => {
                    return false;
                }
            }
            ip = instruction.next_ip() as u32;
        }
    }

    /// Adopts unreferenced code sitting between known functions.
    fn adopt_orphans(&mut self) {
        let mut rejected = HashSet::new();
        loop {
            self.classify_gaps();
            let candidates: Vec<Gap> = self
                .runs(UNKNOWN)
                .into_iter()
                .filter(|gap| !rejected.contains(&gap.start))
                .collect();
            let mut adopted = false;
            for gap in candidates {
                if self.plausible_code(&gap) {
                    self.orphans.insert(gap.start);
                    self.speculative.push_back(gap.start);
                    adopted = true;
                } else {
                    rejected.insert(gap.start);
                }
            }
            if !adopted {
                return;
            }
            self.run();
        }
    }

    /// Successors of the instruction at `ip`, recomputed from the final state.
    fn successors(&self, instruction: &Instruction) -> Flow {
        let ip = instruction.ip() as u32;
        let direct = || {
            let target = instruction.near_branch_target() as u32;
            (matches!(
                instruction.op0_kind(),
                OpKind::NearBranch16 | OpKind::NearBranch32
            ) && self.in_text(target))
            .then_some(target)
        };
        match instruction.flow_control() {
            FlowControl::Next | FlowControl::Call | FlowControl::IndirectCall => Flow {
                falls_through: true,
                targets: Vec::new(),
            },
            FlowControl::ConditionalBranch => Flow {
                falls_through: true,
                targets: direct().into_iter().collect(),
            },
            FlowControl::UnconditionalBranch => Flow {
                falls_through: false,
                targets: direct().into_iter().collect(),
            },
            FlowControl::IndirectBranch => Flow {
                falls_through: false,
                targets: self.tables.get(&ip).cloned().unwrap_or_default(),
            },
            _ => Flow {
                falls_through: false,
                targets: Vec::new(),
            },
        }
    }

    fn is_code(&self, va: u32) -> bool {
        self.in_text(va) && self.kind[self.index(va)] == INSN_START
    }
}

/// Instructions that a 32-bit Windows program built by this compiler never
/// executes. Meeting one means the bytes are data, not code.
fn is_unexpected(instruction: &Instruction) -> bool {
    use Mnemonic::*;
    matches!(
        instruction.mnemonic(),
        Iret | Iretd
            | Retf
            | In
            | Out
            | Insb
            | Insw
            | Insd
            | Outsb
            | Outsw
            | Outsd
            | Hlt
            | Cli
            | Sti
            | Aaa
            | Aas
            | Aam
            | Aad
            | Daa
            | Das
            | Into
            | Bound
            | Arpl
            | Lds
            | Les
            | Lfs
            | Lgs
            | Lss
            | Int
            | Int1
            | Salc
            | Lahf
            | Enter
            | Loop
            | Loope
            | Loopne
            | Jecxz
            | Jcxz
    ) || matches!(
        instruction.segment_prefix(),
        Register::ES | Register::CS | Register::SS | Register::DS | Register::GS
    ) || (0..instruction.op_count()).any(|i| {
        instruction.op_kind(i) == OpKind::Register && instruction.op_register(i).is_segment_register()
    })
}

/// Analyses the code section of `image`.
///
/// Returns `None` if the image has no executable section at its entry point.
pub fn analyze(image: &Image) -> Option<Analysis> {
    let text = image.code_section()?;
    let mut d = Discovery {
        image,
        text,
        kind: vec![UNKNOWN; text.data.len()],
        fallen_into: vec![false; text.data.len()],
        certain: VecDeque::from([image.entry]),
        speculative: VecDeque::new(),
        call_targets: BTreeSet::new(),
        address_taken: BTreeSet::new(),
        orphans: BTreeSet::new(),
        local_targets: HashSet::new(),
        data_refs: BTreeSet::new(),
        tables: HashMap::new(),
        unresolved_indirect_jumps: Vec::new(),
        conflicts: Vec::new(),
        import_uses: HashMap::new(),
        direct_calls: 0,
        indirect_calls: 0,
    };

    // Code pointers stored in data: vtables, callback tables, handler tables.
    // Fixups inside the code section are instruction operands or jump tables
    // and are classified when the code around them is decoded.
    let text_end = text.va + text.data.len() as u32;
    for &reloc in &image.relocs {
        if (text.va..text_end).contains(&reloc) {
            continue;
        }
        if let Some(target) = image.read_u32(reloc).filter(|&t| d.in_text(t))
            && d.address_taken.insert(target)
        {
            d.speculative.push_back(target);
        }
    }

    d.run();
    d.adopt_orphans();

    // A stored pointer into the middle of a function is a label (exception
    // handler, computed goto), not a function of its own.
    let mut entries: BTreeMap<u32, EntryKind> = BTreeMap::new();
    let mut address_taken_labels = Vec::new();
    for &va in &d.address_taken {
        if !d.is_code(va) {
            continue;
        }
        if d.fallen_into[d.index(va)] || d.local_targets.contains(&va) {
            address_taken_labels.push(va);
        } else {
            entries.insert(va, EntryKind::AddressTaken);
        }
    }
    for &va in &d.orphans {
        if d.is_code(va) {
            entries.insert(va, EntryKind::Orphan);
        }
    }
    for &va in &d.call_targets {
        if d.is_code(va) {
            entries.insert(va, EntryKind::Called);
        }
    }
    if d.is_code(image.entry) {
        entries.insert(image.entry, EntryKind::Entry);
    }

    let mut owners = vec![0u8; d.kind.len()];
    let mut mnemonics: BTreeMap<String, u32> = BTreeMap::new();
    let mut functions = Vec::with_capacity(entries.len());
    for (&entry, &kind) in &entries {
        let mut function = Function {
            entry,
            kind,
            instructions: 0,
            bytes: 0,
            start: entry,
            end: entry,
            tail_transfers: 0,
        };
        let mut seen = HashSet::from([entry]);
        let mut stack = vec![entry];
        while let Some(ip) = stack.pop() {
            let Some((instruction, _)) = d.decode(ip) else {
                continue;
            };
            let at = d.index(ip);
            if owners[at] == 0 {
                *mnemonics
                    .entry(format!("{:?}", instruction.mnemonic()).to_lowercase())
                    .or_default() += 1;
            }
            owners[at] = owners[at].saturating_add(1);
            function.instructions += 1;
            function.bytes += instruction.len() as u32;
            function.start = function.start.min(ip);
            function.end = function.end.max(instruction.next_ip() as u32);
            let flow = d.successors(&instruction);
            let fall = flow.falls_through.then_some(instruction.next_ip() as u32);
            for successor in flow.targets.into_iter().chain(fall) {
                if !d.is_code(successor) {
                    continue;
                }
                if successor != entry && entries.contains_key(&successor) {
                    function.tail_transfers += 1;
                } else if seen.insert(successor) {
                    stack.push(successor);
                }
            }
        }
        functions.push(function);
    }

    let mut bytes = ByteCounts::default();
    for &k in &d.kind {
        match k {
            INSN_START | INSN_TAIL => bytes.code += 1,
            JUMP_TABLE => bytes.jump_tables += 1,
            PADDING => bytes.padding += 1,
            DATA => bytes.data += 1,
            _ => bytes.unknown += 1,
        }
    }

    let mut imports: Vec<ImportUse> = image
        .imports
        .iter()
        .map(|import| {
            let [call_sites, jump_sites, other_refs] =
                d.import_uses.get(&import.iat_va).copied().unwrap_or_default();
            ImportUse {
                dll: import.dll.clone(),
                name: import.name.to_string(),
                call_sites,
                jump_sites,
                other_refs,
            }
        })
        .collect();
    imports.sort_by(|a, b| (&a.dll, &a.name).cmp(&(&b.dll, &b.name)));

    d.unresolved_indirect_jumps.sort_unstable();
    d.conflicts.sort_by_key(|c| c.at);
    Some(Analysis {
        code_start: text.va,
        code_len: text.data.len() as u32,
        bytes,
        instructions: d.kind.iter().filter(|&&k| k == INSN_START).count() as u32,
        shared_instructions: owners.iter().filter(|&&n| n > 1).count() as u32,
        functions,
        address_taken_labels,
        jump_tables: d.tables.len() as u32,
        jump_table_entries: d.tables.values().map(|t| t.len() as u32).sum(),
        direct_calls: d.direct_calls,
        indirect_calls: d.indirect_calls,
        unknown_gaps: d.runs(UNKNOWN),
        unresolved_indirect_jumps: d.unresolved_indirect_jumps,
        conflicts: d.conflicts,
        imports,
        mnemonics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::SCN_MEM_EXECUTE;

    const BASE: u32 = 0x40_1000;

    fn image(code: &[u8], relocs: &[u32]) -> Image {
        Image {
            base: 0x40_0000,
            entry: BASE,
            sections: vec![Section {
                name: ".text".into(),
                va: BASE,
                virtual_size: code.len() as u32,
                flags: SCN_MEM_EXECUTE,
                data: code.to_vec(),
            }],
            relocs: relocs.to_vec(),
            ..Image::default()
        }
    }

    fn entries(analysis: &Analysis) -> Vec<(u32, EntryKind)> {
        analysis.functions.iter().map(|f| (f.entry, f.kind)).collect()
    }

    #[test]
    fn follows_calls_and_marks_padding() {
        #[rustfmt::skip]
        let code = [
            0xE8, 0x06, 0x00, 0x00, 0x00, // 1000: call 100B
            0xC3,                         // 1005: ret
            0x90, 0x90, 0x90, 0x90, 0x90, // 1006: padding
            0x31, 0xC0,                   // 100B: xor eax, eax
            0xC3,                         // 100D: ret
        ];
        let analysis = analyze(&image(&code, &[])).unwrap();
        assert_eq!(
            entries(&analysis),
            [(BASE, EntryKind::Entry), (BASE + 0xB, EntryKind::Called)]
        );
        assert_eq!(analysis.bytes.code, 9);
        assert_eq!(analysis.bytes.padding, 5);
        assert_eq!(analysis.bytes.unknown, 0);
        assert_eq!(analysis.direct_calls, 1);
        assert!(analysis.conflicts.is_empty());
    }

    #[test]
    fn recovers_jump_table_from_relocations() {
        #[rustfmt::skip]
        let code = [
            0xFF, 0x24, 0x85, 0x0C, 0x10, 0x40, 0x00, // 1000: jmp [eax*4 + 0x40100C]
            0x90,                                     // 1007: padding
            0x40, 0xC3,                               // 1008: inc eax; ret
            0x48, 0xC3,                               // 100A: dec eax; ret
            0x08, 0x10, 0x40, 0x00,                   // 100C: table[0] = 1008
            0x0A, 0x10, 0x40, 0x00,                   // 1010: table[1] = 100A
        ];
        let analysis = analyze(&image(&code, &[BASE + 3, BASE + 0xC, BASE + 0x10])).unwrap();
        assert_eq!(analysis.jump_tables, 1);
        assert_eq!(analysis.jump_table_entries, 2);
        assert_eq!(analysis.bytes.jump_tables, 8);
        assert_eq!(analysis.bytes.unknown, 0);
        assert_eq!(entries(&analysis), [(BASE, EntryKind::Entry)]);
        assert_eq!(analysis.functions[0].instructions, 5);
        assert!(analysis.unresolved_indirect_jumps.is_empty());
    }

    #[test]
    fn relocated_immediate_is_a_function_and_displacement_is_data() {
        #[rustfmt::skip]
        let code = [
            0x68, 0x10, 0x10, 0x40, 0x00,             // 1000: push 0x401010
            0x8A, 0x80, 0x14, 0x10, 0x40, 0x00,       // 1005: mov al, [eax + 0x401014]
            0xC3,                                     // 100B: ret
            0x90, 0x90, 0x90, 0x90,                   // 100C: padding
            0x31, 0xC0, 0xC3,                         // 1010: xor eax, eax; ret
            0x90,                                     // 1013: padding
            0x00, 0x01, 0x02, 0x03,                   // 1014: byte table
        ];
        let analysis = analyze(&image(&code, &[BASE + 1, BASE + 7])).unwrap();
        assert_eq!(
            entries(&analysis),
            [(BASE, EntryKind::Entry), (BASE + 0x10, EntryKind::AddressTaken)]
        );
        assert_eq!(analysis.bytes.data, 4);
        assert_eq!(analysis.bytes.unknown, 0);
    }

    #[test]
    fn function_after_a_data_table_is_still_found() {
        #[rustfmt::skip]
        let mut code = vec![
            0x8A, 0x80, 0x08, 0x10, 0x40, 0x00,       // 1000: mov al, [eax + 0x401008]
            0xC3,                                     // 1006: ret
            0x90,                                     // 1007: padding
            0x00, 0x01, 0x02, 0x03,                   // 1008: byte table
            0x90, 0x90, 0x90, 0x90,                   // 100C: padding to the boundary
        ];
        code.extend([0x31, 0xC0, 0xC3]); // 1010: unreferenced function
        let analysis = analyze(&image(&code, &[BASE + 2])).unwrap();
        assert_eq!(
            entries(&analysis),
            [(BASE, EntryKind::Entry), (BASE + 0x10, EntryKind::Orphan)]
        );
        assert_eq!(analysis.bytes.data, 4);
        assert_eq!(analysis.bytes.unknown, 0);
    }

    #[test]
    fn adopts_aligned_unreferenced_code_only() {
        let mut code = vec![0xC3]; // 1000: ret
        code.extend([0x90; 15]); // 1001: padding up to the next 16-byte boundary
        code.extend([0x31, 0xC0, 0xC3]); // 1010: orphan: xor eax, eax; ret
        code.push(0x90); // 1013: padding
        code.extend([0x40, 0xC3]); // 1014: decodes (inc eax; ret) but is unaligned
        let analysis = analyze(&image(&code, &[])).unwrap();
        assert_eq!(
            entries(&analysis),
            [(BASE, EntryKind::Entry), (BASE + 0x10, EntryKind::Orphan)]
        );
        assert_eq!(analysis.bytes.unknown, 2);
        assert_eq!(analysis.unknown_gaps.len(), 1);
        assert_eq!(analysis.unknown_gaps[0].start, BASE + 0x14);
    }

    #[test]
    fn jump_table_indexed_from_one() {
        #[rustfmt::skip]
        let code = [
            0xFF, 0x24, 0x85, 0x08, 0x10, 0x40, 0x00, // 1000: jmp [eax*4 + 0x401008]
            0x90,                                     // 1007
            0x40, 0xC3, 0x90, 0x90,                   // 1008: nominal base: code, no relocation
            0x08, 0x10, 0x40, 0x00,                   // 100C: table[1] = 1008
        ];
        let analysis = analyze(&image(&code, &[BASE + 3, BASE + 0xC])).unwrap();
        assert_eq!(analysis.jump_table_entries, 1);
        assert_eq!(analysis.bytes.unknown, 0);
    }

    #[test]
    fn pointer_into_a_function_body_is_a_label() {
        #[rustfmt::skip]
        let code = [
            0x68, 0x07, 0x10, 0x40, 0x00, // 1000: push 0x401007
            0x40,                         // 1005: inc eax
            0x40,                         // 1006: inc eax
            0x48,                         // 1007: dec eax   <- address taken, fallen into
            0xC3,                         // 1008: ret
        ];
        let analysis = analyze(&image(&code, &[BASE + 1])).unwrap();
        assert_eq!(entries(&analysis), [(BASE, EntryKind::Entry)]);
        assert_eq!(analysis.address_taken_labels, [BASE + 7]);
    }
}
