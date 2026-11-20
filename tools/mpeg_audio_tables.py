#!/usr/bin/env python3
"""Writes crates/mpeg/src/audio_tables.rs from the audio part of pl_mpeg.

The tables are those of ISO/IEC 11172-3 (MPEG-1 audio, Layer II) in the
form pl_mpeg gives them, and the 32-point transform of the synthesis
filter, which in pl_mpeg is a long run of sums and products: it is
converted statement by statement, so that no number is copied by hand.
(Its input is floating point here, where pl_mpeg's is integer: see
`crates/mpeg/src/audio.rs` on requantisation.)

    python3 tools/mpeg_audio_tables.py ref/mpeg/pl_mpeg.h > crates/mpeg/src/audio_tables.rs

pl_mpeg is by Dominic Szablewski, MIT license: https://github.com/phoboslab/pl_mpeg
Its audio decoder is based on kjmp2 by Martin J. Fiedler.
"""
import re
import sys

source = open(sys.argv[1]).read()
MACROS = {name: eval(value) for name, value in re.findall(r"#define (PLM_AUDIO_QUANT_TAB_[A-D]) (\([^)]*\)|\d+)", source)}


def body(name, dims=r"\[\]"):
    match = re.search(r"\b" + name + dims + r"\s*=\s*\{(.*?)\n\};", source, re.S)
    assert match, name
    return re.sub(r"//[^\n]*", "", match.group(1))


def value(item):
    return MACROS[item] if item in MACROS else (float(item) if "." in item else int(item, 0))


def flat(text):
    return [value(item.strip()) for item in text.replace("\n", " ").split(",") if item.strip()]


def nested(name, dims):
    return [flat(row) for row in re.findall(r"\{([^{}]*)\}", body(name, dims))]


def lines(items, per_row, indent="    "):
    return "\n".join(indent + ", ".join(items[start:start + per_row]) + "," for start in range(0, len(items), per_row))


def table(doc, name, kind, rows, width):
    print(f"/// {doc}\npub static {name}: [[{kind}; {width}]; {len(rows)}] = [")
    for row in rows:
        padded = row + [0] * (width - len(row))
        print("    [" + ", ".join(str(item) for item in padded) + "],")
    print("];\n")


print("""//! Tables of ISO/IEC 11172-3, Layer II, and the transform of the synthesis
//! filter. Written by `tools/mpeg_audio_tables.py` from pl_mpeg (Dominic
//! Szablewski, MIT license; its audio decoder is based on kjmp2 by Martin
//! J. Fiedler); do not edit.
""")

window = flat(body("PLM_AUDIO_SYNTHESIS_WINDOW"))
assert len(window) == 512
print("/// The window of the synthesis filter.")
print(f"pub static SYNTHESIS_WINDOW: [f32; 512] = [\n{lines([repr(v) for v in window], 8)}\n];\n")

rates = flat(body("PLM_AUDIO_SAMPLE_RATE"))
print(f"/// Samples a second by the header's index (MPEG-1).\npub static SAMPLE_RATE: [u32; 3] = {rates[:3]};\n")
bit_rates = flat(body("PLM_AUDIO_BIT_RATE"))
print(f"/// Thousands of bits a second by the header's index (MPEG-1).\npub static BIT_RATE: [u32; 14] = {bit_rates[:14]};\n")

table("Bit rate class by channels (one, two) and bit rate index.", "QUANT_LUT_STEP_1", "u8",
      nested("PLM_AUDIO_QUANT_LUT_STEP_1", r"\[2\]\[16\]"), 16)
table("By bit rate class and sample rate: the number of subbands in the low six bits,\n/// and above them which row of the next table.",
      "QUANT_LUT_STEP_2", "u8", nested("QUANT_LUT_STEP_2", r"\[3\]\[3\]"), 3)
step_3 = nested("PLM_AUDIO_QUANT_LUT_STEP_3", r"\[3\]\[32\]")
table("By subband: the bits of its allocation (high four bits) and the row of the next\n/// table (low four).",
      "QUANT_LUT_STEP_3", "u8", step_3, 32)
table("By row and allocation: zero for no samples, or one more than the index of a quantiser.",
      "QUANT_LUT_STEP_4", "u8", nested("PLM_AUDIO_QUANT_LUT_STEP_4", r"\[6\]\[16\]"), 16)
quantisers = nested("PLM_AUDIO_QUANT_TAB", r"\[\]")
assert len(quantisers) == 17
print("/// The quantisers: levels, whether three samples share one code, and the bits of a\n/// code.")
print("pub static QUANTISERS: [(i32, bool, u32); 17] = [")
for levels, group, bits in quantisers:
    print(f"    ({levels}, {'true' if group else 'false'}, {bits}),")
print("];\n")

# The transform.
function = re.search(r"void plm_audio_idct36\(int s\[32\]\[3\], int ss, float \*d, int dp\) \{(.*?)\n\}", source, re.S)
assert function
statements = []
for statement in function.group(1).replace("\n", " ").split(";"):
    statement = statement.strip()
    if not statement:
        continue
    if statement.startswith("float "):
        names = [name.strip() for name in statement[6:].split(",")]
        continue
    # The subband samples are floats here (see `audio.rs`), so the casts go.
    statement = re.sub(r"\(float\)\(([^()]*)\)", r"(\1)", statement)
    statement = re.sub(r"(\d+\.\d+)f\b", r"\1", statement)
    # a = b = value
    parts = [part.strip() for part in re.split(r"(?<![+\-*/<>=!])=(?!=)", statement)]
    if len(parts) > 2:
        statements.extend(f"{target} = {parts[-1]}" for target in parts[:-1])
    else:
        statements.append(statement)
assert len(names) == 33 and len(statements) > 250, (len(names), len(statements))
print("""/// The 32-point transform of the synthesis filter: one sample of each of
/// the 32 subbands in (`s[..][ss]`), 64 values of the filter's vector out
/// (`d[dp..dp + 64]`).
#[rustfmt::skip]
#[allow(clippy::all, unused_assignments, unused_parens)]
pub fn idct36(s: &[[f32; 3]; 32], ss: usize, d: &mut [f32], dp: usize) {""")
print("    let (" + ", ".join("mut " + name for name in names) + "): (" + ", ".join(["f32"] * 33) + ");")
for statement in statements:
    print(f"    {statement};")
print("}")
