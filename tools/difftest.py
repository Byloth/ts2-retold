#!/usr/bin/env python3
"""Replay andy's differential test cases on Unicorn and compare.

Usage: .venv/bin/python tools/difftest.py out/difftest.jsonl [--jobs N] [--show N]

Each line of the input is one instruction with an initial machine state and
the state andy's interpreter ended in (see crates/recomp/src/difftest.rs).
Unicorn (QEMU's CPU core, GPL-2.0) runs as a separate process and is used
only as a test oracle; nothing of it is linked into the project.
"""

import argparse
import json
import math
import struct
import sys
from collections import Counter, defaultdict
from concurrent.futures import ProcessPoolExecutor

from unicorn import (UC_ARCH_X86, UC_HOOK_MEM_FETCH_UNMAPPED, UC_HOOK_MEM_READ_UNMAPPED,
                     UC_HOOK_MEM_WRITE, UC_HOOK_MEM_WRITE_UNMAPPED, UC_MODE_32, Uc, UcError)
from unicorn import x86_const as x86

PAGE = 0x1000
GPR = [x86.UC_X86_REG_EAX, x86.UC_X86_REG_ECX, x86.UC_X86_REG_EDX, x86.UC_X86_REG_EBX,
       x86.UC_X86_REG_ESP, x86.UC_X86_REG_EBP, x86.UC_X86_REG_ESI, x86.UC_X86_REG_EDI]
FLAG_MASK = 0xCD5  # CF PF AF ZF SF DF OF
STATUS_MASK = 0x4500  # C3 C2 C0 (C1 reports rounding on real hardware)
STATUS_WITH_C1 = {"fprem", "fprem1", "fxam"}  # here C1 carries a result
# Results that are not exact in an f64 model, or that depend on the quality of
# the host's maths library: compared with a tolerance.
APPROXIMATE = {"fsin", "fcos", "fptan", "fpatan", "fyl2x", "f2xm1", "fscale",
               "fldpi", "fldl2e", "fldlg2", "fldln2"}
# The oracle (QEMU 5.0's helpers) is not a reference for these: fprem and
# fprem1 are computed as st0 - st1 * quotient in double arithmetic, with large
# cancellation error, a wrong result when |st0| < |st1| in fprem1, and partial
# reduction from an exponent gap of 53 instead of 64. They are skipped here
# and covered by unit tests against exact arithmetic instead.
ORACLE_UNRELIABLE = {"fprem", "fprem1"}
# pow(2, x) - 1 in the oracle loses digits for small x; ours uses expm1.
LOOSE = {"f2xm1": 1e-9}
# Condition codes are documented as undefined after these.
STATUS_UNDEFINED = {"fsin", "fcos", "fptan", "fpatan", "fyl2x", "f2xm1", "fscale", "fsqrt",
                    "frndint", "fild", "fld", "fst", "fstp", "fistp", "fadd", "faddp", "fsub",
                    "fsubp", "fsubr", "fsubrp", "fmul", "fmulp", "fdiv", "fdivp", "fdivr",
                    "fdivrp", "fiadd", "fidiv", "fimul", "fabs", "fchs", "fxch", "fld1", "fldz",
                    "fldpi", "fldl2e", "fldlg2", "fldln2"}


def make_pattern():
    state = 0x12345678
    out = bytearray(PAGE)
    for i in range(PAGE):
        state = (state * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = state >> 24
    return bytes(out)


PATTERN = make_pattern()


def page_bytes(number):
    rotation = ((number * 2654435761) & 0xFFFFFFFF) >> 20
    return PATTERN[rotation:] + PATTERN[:rotation]


def f64_to_f80(bits):
    sign = (bits >> 63) << 15
    exponent = (bits >> 52) & 0x7FF
    fraction = bits & ((1 << 52) - 1)
    if exponent == 0x7FF:
        payload = 0 if fraction == 0 else (fraction << 11) | (1 << 62)
        return (1 << 63) | payload, sign | 0x7FFF
    if exponent == 0 and fraction == 0:
        return 0, sign
    if exponent == 0:
        mantissa, unbiased = fraction << 11, -1022
    else:
        mantissa, unbiased = (fraction | (1 << 52)) << 11, exponent - 1023
    while not mantissa >> 63:
        mantissa <<= 1
        unbiased -= 1
    return mantissa, sign | (unbiased + 16383)


def f80_to_f64(mantissa, sign_exponent):
    """Returns (value, exactly representable as f64)."""
    negative = sign_exponent & 0x8000
    exponent = sign_exponent & 0x7FFF
    if exponent == 0x7FFF:
        value = math.inf if (mantissa << 1) & 0xFFFFFFFFFFFFFFFF == 0 else math.nan
        return (-value if negative else value), True
    if mantissa == 0:
        return (-0.0 if negative else 0.0), True
    exact = mantissa & 0x7FF == 0
    try:
        value = math.ldexp(mantissa, max(exponent, 1) - 16383 - 63)
    except OverflowError:
        value, exact = math.inf, False
    return (-value if negative else value), exact


def bits_of(value):
    return struct.unpack("<Q", struct.pack("<d", value))[0]


class Machine:
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.mapped = set()
        self.written = []
        unmapped = UC_HOOK_MEM_READ_UNMAPPED | UC_HOOK_MEM_WRITE_UNMAPPED | UC_HOOK_MEM_FETCH_UNMAPPED
        self.uc.hook_add(unmapped, self.on_unmapped)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.on_write)

    def map_page(self, number):
        if number not in self.mapped:
            self.uc.mem_map(number * PAGE, PAGE)
            self.uc.mem_write(number * PAGE, page_bytes(number))
            self.mapped.add(number)

    def map_range(self, address, size):
        first = address // PAGE
        last = ((address + size - 1) & 0xFFFFFFFF) // PAGE
        self.map_page(first)
        self.map_page(last)

    def on_unmapped(self, uc, access, address, size, value, user):
        self.map_range(address & 0xFFFFFFFF, max(size, 1))
        return True

    def on_write(self, uc, access, address, size, value, user):
        self.written.append((address & 0xFFFFFFFF, size))

    def write(self, address, data):
        for i, byte in enumerate(data):  # byte-wise: the range may wrap
            at = (address + i) & 0xFFFFFFFF
            self.map_page(at // PAGE)
            self.uc.mem_write(at, bytes([byte]))

    def read(self, address, size):
        out = bytearray()
        for i in range(size):
            at = (address + i) & 0xFFFFFFFF
            if at // PAGE not in self.mapped:
                return None
            out += self.uc.mem_read(at, 1)
        return bytes(out)

    def reset(self):
        for number in self.mapped:
            self.uc.mem_unmap(number * PAGE, PAGE)
        self.mapped.clear()
        self.written.clear()

    def run(self, case):
        uc = self.uc
        initial = case["initial"]
        code = bytes.fromhex(case["code"])
        va = case["va"]
        for address, data in case["preset"]:
            self.write(address, bytes.fromhex(data))
        self.write(va, code)
        for reg, value in zip(GPR, initial["regs"]):
            uc.reg_write(reg, value)
        uc.reg_write(x86.UC_X86_REG_EFLAGS, initial["eflags"])
        fpu = initial["fpu"]
        uc.reg_write(x86.UC_X86_REG_FPCW, fpu["control"])
        uc.reg_write(x86.UC_X86_REG_FPSW, fpu["status"] | fpu["top"] << 11)
        tags = 0
        for slot, bits in enumerate(fpu["regs"]):
            if bits is None:
                tags |= 3 << (2 * slot)
                uc.reg_write(x86.UC_X86_REG_FP0 + slot, (0, 0))
            else:
                uc.reg_write(x86.UC_X86_REG_FP0 + slot, f64_to_f80(int(bits, 16)))
        uc.reg_write(x86.UC_X86_REG_FPTAG, tags)
        self.written.clear()
        fault = False
        try:
            if case["rep"]:
                uc.emu_start(va, va + len(code))
            else:
                uc.emu_start(va, 0, count=1)
        except UcError:
            fault = True
        # The oracle does not always report a CPU exception as an error;
        # sometimes it just stops with the instruction not executed.
        expect = case["expect"]
        if uc.reg_read(x86.UC_X86_REG_EIP) == va and (expect["fault"] or expect["eip"] != va):
            fault = True
        return fault

    def compare(self, case, fault):
        """Returns (list of differences, compared approximately?)."""
        uc = self.uc
        expect = case["expect"]
        mnemonic = case["mnemonic"]
        if fault or expect["fault"]:
            return ([] if fault == expect["fault"] else [f"fault: ours {expect['fault']}, oracle {fault}"]), False
        diffs = []
        approximate = False
        regs = [uc.reg_read(reg) for reg in GPR]
        for name, ours, theirs in zip("eax ecx edx ebx esp ebp esi edi".split(), expect["regs"], regs):
            if ours != theirs:
                diffs.append(f"{name}: ours {ours:#x}, oracle {theirs:#x}")
        eip = uc.reg_read(x86.UC_X86_REG_EIP)
        if eip != expect["eip"]:
            diffs.append(f"eip: ours {expect['eip']:#x}, oracle {eip:#x}")
        mask = FLAG_MASK & ~case["undefined"]
        eflags = uc.reg_read(x86.UC_X86_REG_EFLAGS)
        if (eflags ^ expect["eflags"]) & mask:
            diffs.append(f"eflags: ours {expect['eflags'] & mask:#x}, oracle {eflags & mask:#x}")

        covered = set()
        for address, data in expect["writes"]:
            data = bytes.fromhex(data)
            covered.update((address + i) & 0xFFFFFFFF for i in range(len(data)))
            theirs = self.read(address, len(data))
            if theirs != data:
                shown = theirs.hex() if theirs is not None else "unmapped"
                diffs.append(f"memory {address:#x}: ours {data.hex()}, oracle {shown}")
        for address, size in self.written:
            if any((address + i) & 0xFFFFFFFF not in covered for i in range(size)):
                diffs.append(f"oracle wrote {size} bytes at {address:#x}, we did not")
                break

        fpu = expect["fpu"]
        status = uc.reg_read(x86.UC_X86_REG_FPSW)
        top = status >> 11 & 7
        if top != fpu["top"]:
            diffs.append(f"fpu top: ours {fpu['top']}, oracle {top}")
        status_mask = STATUS_MASK | (0x200 if mnemonic in STATUS_WITH_C1 else 0)
        if mnemonic not in STATUS_UNDEFINED and (status ^ fpu["status"]) & status_mask:
            diffs.append(f"fpu status: ours {fpu['status'] & status_mask:#x}, oracle {status & status_mask:#x}")
        control = uc.reg_read(x86.UC_X86_REG_FPCW)
        if control != fpu["control"]:
            diffs.append(f"fpu control: ours {fpu['control']:#x}, oracle {control:#x}")
        for slot, bits in enumerate(fpu["regs"]):
            if bits is None:
                continue
            ours = struct.unpack("<d", struct.pack("<Q", int(bits, 16)))[0]
            theirs, exact = f80_to_f64(*uc.reg_read(x86.UC_X86_REG_FP0 + slot))
            if math.isnan(ours) and math.isnan(theirs):
                continue
            if exact and mnemonic not in APPROXIMATE and not expect["inexact"]:
                if bits_of(ours) != bits_of(theirs):
                    diffs.append(f"fpu reg {slot}: ours {ours!r}, oracle {theirs!r}")
            else:
                approximate = True
                # The oracle does not round library functions to single
                # precision when the control word asks for it; we do.
                single = fpu["control"] & 0x300 == 0
                tolerance = 2e-7 if single else LOOSE.get(mnemonic, 1e-12)
                if not math.isclose(ours, theirs, rel_tol=tolerance, abs_tol=1e-300):
                    diffs.append(f"fpu reg {slot} (approx): ours {ours!r}, oracle {theirs!r}")
        return diffs, approximate


def run_chunk(lines):
    machine = Machine()
    results = []
    for line in lines:
        case = json.loads(line)
        if case["mnemonic"] in ORACLE_UNRELIABLE:
            results.append((case["mnemonic"], case["code"], case["va"], None, False, False))
            continue
        fault = machine.run(case)
        diffs, approximate = machine.compare(case, fault)
        machine.reset()
        results.append((case["mnemonic"], case["code"], case["va"], diffs, approximate, fault))
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("cases")
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--show", type=int, default=3, help="failing cases to print per mnemonic")
    parser.add_argument("--only", help="comma-separated mnemonics to run")
    args = parser.parse_args()

    only = set(args.only.split(",")) if args.only else None
    with open(args.cases) as handle:
        lines = [line for line in handle if only is None or json.loads(line)["mnemonic"] in only]
    size = max(1, min(2000, len(lines) // (args.jobs * 4) + 1))
    chunks = [lines[i:i + size] for i in range(0, len(lines), size)]

    total = Counter()
    failed = Counter()
    skipped = Counter()
    failed_encodings = defaultdict(set)
    approximate = faults = 0
    examples = defaultdict(list)
    with ProcessPoolExecutor(args.jobs) as pool:
        for results in pool.map(run_chunk, chunks):
            for mnemonic, code, va, diffs, approx, fault in results:
                if diffs is None:
                    skipped[mnemonic] += 1
                    continue
                total[mnemonic] += 1
                approximate += approx
                faults += fault
                if diffs:
                    failed[mnemonic] += 1
                    failed_encodings[mnemonic].add(code)
                    if len(examples[mnemonic]) < args.show:
                        examples[mnemonic].append((code, va, diffs))

    cases = sum(total.values())
    bad = sum(failed.values())
    print(f"cases {cases}, mnemonics {len(total)}, agreeing {cases - bad}, differing {bad}")
    print(f"compared approximately {approximate}, faulting on both sides {faults}")
    if skipped:
        names = ", ".join(f"{name} ({count})" for name, count in sorted(skipped.items()))
        print(f"not compared, oracle unreliable: {names}")
    for mnemonic, count in failed.most_common():
        print(f"\n{mnemonic}: {count} of {total[mnemonic]} cases differ "
              f"({len(failed_encodings[mnemonic])} encodings)")
        for code, va, diffs in examples[mnemonic]:
            print(f"  {code} at {va:#x}")
            for diff in diffs[:4]:
                print(f"    {diff}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
