#!/usr/bin/env python3
"""Writes crates/mpeg/src/tables.rs from the tables of pl_mpeg.

The tables are those of ISO/IEC 11172-2 (MPEG-1 video) in the form pl_mpeg
gives them: quantisation matrices, the zig-zag order, and the variable-length
codes as binary trees. They are converted by program so that no number is
copied by hand.

    python3 tools/mpeg_tables.py ref/mpeg/pl_mpeg.h > crates/mpeg/src/tables.rs

pl_mpeg is by Dominic Szablewski, MIT license: https://github.com/phoboslab/pl_mpeg
"""
import re
import sys

source = open(sys.argv[1]).read()


def body(name):
    match = re.search(r"\b" + name + r"\[\]\s*=\s*\{(.*?)\n\};", source, re.S)
    assert match, name
    return re.sub(r"//[^\n]*", "", match.group(1))


def numbers(name):
    return [int(item, 0) for item in body(name).replace("\n", " ").split(",") if item.strip()]


def pairs(name):
    found = re.findall(r"\{\s*([^,{}]+?)\s*,\s*([^,{}]+?)\s*\}", body(name))
    return [(eval(index), eval(value)) for index, value in found]


def rows(items, per_row):
    lines = []
    for start in range(0, len(items), per_row):
        lines.append("    " + ", ".join(items[start:start + per_row]) + ",")
    return "\n".join(lines)


print("""//! Tables of ISO/IEC 11172-2. Written by `tools/mpeg_tables.py` from the
//! tables of pl_mpeg (Dominic Szablewski, MIT license); do not edit.
//!
//! A variable-length code is a binary tree in an array: starting at entry 0,
//! each bit read selects entry `index + bit`; an index of zero or less ends
//! the code, whose value is the entry's second number.
""")
for name, rust, doc in [
    ("PLM_VIDEO_ZIG_ZAG", "ZIG_ZAG", "The position in a block of each coefficient in the order they are coded."),
    ("PLM_VIDEO_INTRA_QUANT_MATRIX", "INTRA_QUANT_MATRIX", "The default quantisation matrix of intra blocks."),
    ("PLM_VIDEO_NON_INTRA_QUANT_MATRIX", "NON_INTRA_QUANT_MATRIX", "The default quantisation matrix of other blocks."),
    ("PLM_VIDEO_PREMULTIPLIER_MATRIX", "PREMULTIPLIER_MATRIX", "Scale factors the inverse transform expects on its input."),
]:
    values = numbers(name)
    assert len(values) == 64, name
    print(f"/// {doc}\npub static {rust}: [u8; 64] = [\n{rows([str(v) for v in values], 8)}\n];\n")

for name, rust, doc in [
    ("PLM_VIDEO_MACROBLOCK_ADDRESS_INCREMENT", "MACROBLOCK_ADDRESS_INCREMENT", "34 is stuffing, 35 an escape that adds 33."),
    ("PLM_VIDEO_MACROBLOCK_TYPE_INTRA", "MACROBLOCK_TYPE_INTRA", "Macroblock types of I pictures."),
    ("PLM_VIDEO_MACROBLOCK_TYPE_PREDICTIVE", "MACROBLOCK_TYPE_PREDICTIVE", "Macroblock types of P pictures."),
    ("PLM_VIDEO_MACROBLOCK_TYPE_B", "MACROBLOCK_TYPE_B", "Macroblock types of B pictures."),
    ("PLM_VIDEO_CODE_BLOCK_PATTERN", "CODE_BLOCK_PATTERN", "Which of the six blocks of a macroblock are coded."),
    ("PLM_VIDEO_MOTION", "MOTION", "Motion vector codes."),
    ("PLM_VIDEO_DCT_SIZE_LUMINANCE", "DCT_SIZE_LUMINANCE", "Bits of the DC difference of a luminance block."),
    ("PLM_VIDEO_DCT_SIZE_CHROMINANCE", "DCT_SIZE_CHROMINANCE", "Bits of the DC difference of a chrominance block."),
    ("PLM_VIDEO_DCT_COEFF", "DCT_COEFF", "Run (high byte) and level (low byte); 1 is the end of the block, -1 an escape."),
]:
    table = pairs(name)
    # The coefficient table's values are unsigned 16-bit in the source; the
    # escape 0xffff is -1 here.
    items = [f"({index}, {value - 0x10000 if value >= 0x8000 else value})" for index, value in table]
    print(f"/// {doc}\npub static {rust}: [(i16, i16); {len(table)}] = [\n{rows(items, 6)}\n];\n")
