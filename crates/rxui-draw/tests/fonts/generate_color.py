"""Generate Astrelis's tiny original geometric COLR v0 fixture.

Requires fonttools 4.60.2 only for regeneration; Rust tests use the committed TTF.
This source and its generated TestColor.ttf are covered by the repository MIT license.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.colorLib.builder import buildCOLR, buildCPAL
from fontTools.ttLib import newTable
from fontTools.ttLib.tables.sbixStrike import Strike
from fontTools.ttLib.tables.sbixGlyph import Glyph
import struct
import zlib


def rectangle(x0, y0, x1, y1):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()
    return pen.glyph()


builder = FontBuilder(1000, isTTF=True)
names = [".notdef", "space", "mask", "color", "red", "green", "bitmap"]
builder.setupGlyphOrder(names)
builder.setupCharacterMap({32: "space", ord("M"): "mask", 0x1F600: "color", 0x1F601: "bitmap"})
builder.setupGlyf({
    ".notdef": rectangle(50, 0, 450, 700),
    "space": TTGlyphPen(None).glyph(),
    "mask": rectangle(0, 0, 1000, 800),
    "color": TTGlyphPen(None).glyph(),
    "bitmap": TTGlyphPen(None).glyph(),
    "red": rectangle(0, 0, 650, 800),
    "green": rectangle(350, 0, 1000, 800),
})
builder.setupHorizontalMetrics({name: (1000 if name != "space" else 400, 350 if name == "green" else 0) for name in names})
builder.setupHorizontalHeader(ascent=800, descent=-200)
builder.setupNameTable({"familyName": "Astrelis Test Color", "styleName": "Regular",
                       "uniqueFontIdentifier": "AstrelisTestColor-Regular",
                       "fullName": "Astrelis Test Color Regular", "psName": "AstrelisTestColor-Regular"})
builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
builder.setupPost()
builder.setupMaxp()
builder.font["COLR"] = buildCOLR({"color": [("red", 0), ("green", 1)]}, version=0)
builder.font["CPAL"] = buildCPAL([[(1., 0., 0., 1.), (0., 1., 0., 0.5)]])


def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))


png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 20, 16, 8, 6, 0, 0, 0))
       + chunk(b"IDAT", zlib.compress((b"\0" + bytes([64, 128, 255, 128]) * 20) * 16))
       + chunk(b"IEND", b""))
builder.font["sbix"] = newTable("sbix")
strike = Strike(ppem=20, resolution=72)
strike.glyphs = {"bitmap": Glyph(glyphName="bitmap", originOffsetX=0, originOffsetY=0,
                                graphicType="png ", imageData=png)}
builder.font["sbix"].strikes = {20: strike}
builder.font["head"].created = builder.font["head"].modified = 3800000000
builder.font.recalcTimestamp = False
builder.save(Path(__file__).with_name("TestColor.ttf"))
