# ADR 0016: Languages and settings — one build, English and Italian

Status: accepted as a direction (2026-11-20, decided by the owner); the
mechanisms are still to be designed

## Context

The game was released as one build per language, each with its own
executable, dubbed movies, speech and graphics (ADR 0003). The project
supports the English reference build. The owner has the Italian disc and
wants Italian too; the remaining languages could not all be found and are
left for later.

The original chooses its screen mode in a start-up window the project
skips (`skip-mode-select`), and has no language choice at all.

## Decision

1. **One build.** The recompiled game is the English reference build.
   Another language is added to it, not built beside it.
2. **English and Italian first.** The other languages wait in the backlog.
3. **A language is a catalogued modification** (ADR 0004) that brings that
   language's texts, movies, speech and graphics to the reference build.
   The material comes from the user's own disc of that language, at build
   time (ADR 0001): none of it is in the repository.
4. **The language will be chosen in the game's Options menu**, together
   with the video settings the backlog already asks for.
5. **Until that menu exists, settings live in a file**, preferably one the
   game already has.

## What is known about the files the game already has

- **`toy2.ini`** is a text file the game reads at start-up: key
  definitions (`DEFKEY`, `KEY`, `JOY`), switches (`LOG`, `QUIET`, `NOMPEG`,
  `HIMPEG`, `CHEAT`), `MESSAGE`, `DEFTEXT`, and **`LANGUAGE`**. The
  reference executable still parses `LANGUAGE` with a two-letter code (`US`,
  `DE`, `FR`, `NL`, `DK`, `SW`, `IT`, `CT`, `FN`, `NW`) into a variable at
  `0x534480` that nothing reads: a leftover. Giving it its meaning back is
  the natural way to say which language is wanted.
- **`toy2.cfg`** is 24 bytes of binary settings the game writes itself.
- **The registry** key holds the paths and `allow32bit`.
- The screen mode has no setting in any of them: the start-up window asks
  every time.

## Open

- How the reference build finds its texts, so that another language's can
  be put in their place; whether the Italian executable differs from the
  English one in anything but texts.
- Where the screen mode and the project's own settings go: new keywords in
  `toy2.ini` (what the game's parser does with a keyword it does not know
  has to be checked first), or a file of the project's own.

## What the Italian disc shows (2026-11-21)

The Italian disc was laid out in the git-ignored `ref/disc-it` and
compared with the reference copy. The first open question is answered,
and the answer makes decision 3 harder than it reads.

**Texts.** They are C strings in the executable's data, referred to from
code and from tables. Both executables keep their relocation table
(about 35,000 entries), which lists every place an absolute address is
written: so every reference to every text is known exactly, without
guessing. A text can be replaced by one of any length by storing the new
one elsewhere and rewriting the places that refer to the old one; tried
by hand on three menu texts, it works.

Of about 2,600 strings referred to in each executable, 2,250 are the same
(file names, messages for the log), and about 350 differ:

- the level texts (hints and dialogue, some 150) correspond one to one;
- the menus do not: the Italian executable draws several of them
  differently, often as two lines where the English has one, and has
  entries the English lacks and the other way round;
- the names of the keys are a table by key code and differ in which keys
  have names;
- the English executable still holds a few texts in German (the video
  options and the messages of saving and loading).

**The typeface.** With Italian texts put into the English executable,
accented letters come out as other signs, and so does the colon: the
English executable's font and its code for drawing text do not have them.

**The code.** The Italian executable is not the English one with other
strings. Of 2,414 functions, about 2,375 are the same once addresses are
set aside, and about 40 differ: the four that draw glyphs (three times
the size in the Italian one), the menus, and others not yet looked at.
The Italian and the French executables in turn differ from each other in
about 20 functions: each language's build has code of its own.

**The files.** Beside the 18 movies: 78 speech files differ and 11 exist
only on the Italian disc; 25 files of the loading and result screens
differ; and so do level files that carry no language (terrain, object
placement, paths in some ten levels): the Italian disc is also a later
revision of the game's data.

So "bringing Italian to the reference build" means one of:

1. **A hybrid**: English code and level data, Italian texts, speech,
   movies and screens, and the Italian executable's text drawing and menu
   layouts rebuilt as modifications of the English functions. One
   simulation for every language, which replays and multiplayer want; a
   game that was never released in this form; much work by hand on some
   40 functions.
2. **Each language its own executable**, recompiled as it was released.
   Faithful to what was played in each country, including each revision's
   own fixes and faults; everything tied to addresses (modifications, and
   later the enhancements) has to exist for every build, which the
   functions' near-identity makes largely mechanical (an address map from
   one build to another can be derived) but which must be verified per
   build; simulations differ between builds, so a replay or a multiplayer
   session belongs to one build.

**Decided by the owner (2026-11-21): the hybrid.** English code and level
data; from the Italian disc the texts, the speech, the movies and the
screens, and the Italian text drawing and menus on top of the English
functions.

## How the hybrid is to be made

Not by rewriting forty functions by hand from a reading of the Italian
ones: by **transplanting**, with the two executables as they are.

1. **An address map between the two builds.** The code of both is cut at
   its relocation sites; the pieces between sites are the same in both
   wherever the code is the same, so aligning the two sequences of pieces
   pairs the sites, and each pair of sites gives a pair of addresses: the
   same variable, function, table or text in the one build and in the
   other. Tried: of 33,186 sites in the English code 33,021 align, giving
   7,384 pairs of addresses, 4 of them with more than one answer. The
   addresses referred to only from tables in the data (about 1,300) are
   to be reached by walking paired tables side by side.
2. **Texts**: where a pair is two different strings, the places that
   refer to the English one are made to refer to the Italian one, stored
   beside the image. The pairs come from the code that uses the texts,
   not from their order in the file.
3. **Functions**: each of the forty that differ is looked at once, to say
   whether its difference is language (text drawing, a menu's layout) or
   revision (which stays English). A function of the first kind is taken
   from the Italian executable as it is, placed beside the English image
   with its addresses translated through the map, and the English
   function's entry leads to it. What it draws is then what the Italian
   game draws, by the Italian game's own code.
4. **Files**: speech, movies and screens are served from the Italian disc
   in place of the English ones, by a list of which files are language
   and which are the revision's level data.

The result is an image the interpreter and the recompiler take like any
other; with the modification off the English image is untouched. The
user's Italian disc is read when the game is built or started; nothing
of it is in the repository, where the transplant is described by
addresses of the two builds.

What the look at the glyph functions already showed: the Italian ones
draw an accented letter as the letter and, above it, an accent that is a
glyph of its own in the same sheet, and set the letters 7 pixels apart
where the English ones use 8, which is how the longer texts fit.

## A language is data (2026-11-21, decided by the owner)

The hybrid above was made for Italian by taking the Italian executable's
own functions wherever they differ for the language. The owner looked at
the result (a main menu entry reading "load-save options" beside one
called "options") and set the direction: **no code of its own for any
language**, which is what the original developers did and what gave each
language its own quirks; a language is to be **data only**, handled by
one mechanism, and French and Spanish are to be handled together with
Italian so that nothing is made for one language.

What the three discs show (French and Spanish laid out in the git-ignored
`ref/disc-fr` and `ref/disc-es`):

- they share one later revision of the game (the engine and sound fixes
  are the same in all three);
- each has code of its own in some fifteen functions, and they differ
  from one another there: the four that draw glyphs differ in where a
  line begins (`0xAF - 4n` in Italian, `0xAA - 4n` in Spanish,
  `(0x28 - n) * 4` in French), in which glyph is which accent (each
  language's typeface texture is laid out differently), and the menus in
  how they split a text that does not fit over two lines;
- the texts and the files transplant by the one mechanism already built:
  345 places for French, 333 for Spanish, with nothing written for them.

So a language is:

1. **its texts**, by place, from that language's executable through the
   pairing; with a short list of corrections the project words itself
   (`language::corrections`), each with its reason;
2. **its files**: speech, movies, textures;
3. **its typefaces as tables**: for each character, which glyph or glyphs
   to draw and how far to move on. Not read by hand out of each build's
   functions: to be had by running those functions, in the interpreter,
   on every character and recording what they draw.

and the code is one: the reference build's menus and screens for every
language, and **one way of drawing text** in place of the reference's
text functions, which takes the tables of 3 and fits a text into the
room the reference's screen gives it. With English tables it must draw
what the reference draws, pixel for pixel. Pictures of a language will
then no longer be those of that language's release where the release
had its own layout; they are checked against the reference's layout
instead.

Changes to the menus the owner asked for, for every language, are
modifications of that one code (ADR 0004): the entry for loading and
saving before the options in the main menu.

State: 1 and 2 are done for Italian, French and Spanish (the
modification `language`, the disc in `TS2_LANGUAGE_DIR` says which). The
one way of drawing text is done for the large typeface (see "The large
typeface done" below); no function of another build is used any more.

## One atlas, one typeface, one function (2026-11-21, decided by the owner)

The owner settled how text is to be drawn: **when the game is built, the
atlas of each typeface is rebuilt from all the languages' discs**, with
every character any of them needs; the built game has one atlas, one
typeface, one function that draws it, and a language changes the text
and nothing else.

What the discs hold (found by looking, 2026-11-21):

- A texture file (`*.ngn`) begins with its pictures as plain bitmap
  files, each with a name; `texNN` is the texture the sprite sheets call
  number NN (`crates/recomp/src/textures.rs` reads them and can put a
  picture back; all 187 texture files of the four discs read and write
  back unchanged).
- There are three typefaces, each a sheet of sprites over a texture:
  - the **large** one of the menus (sheet 0x32, cells of 32 by 32 in
    `tex31` of the front end's texture files). Its first 48 cells are the
    same picture on all four discs: the letters, the digits and
    `. , ; : \ / ' ! ? ( )`. The English code draws only the letters and
    the apostrophe of these. The last 16 cells differ by language: two
    arrows everywhere, then accents and `¿ ¡ Ø Æ Œ Ç` or a dash, laid out
    differently in each. Taken together they are 21 different cell
    pictures, more than 16: the same sign is drawn slightly differently
    on different discs, so the atlas has to be put together by which
    sign a cell is, not by its pixels. The Italian disc even carries the
    Spanish page in three of its files;
  - the **small** one (sheet 0x43, cells of 16 by 24 in `tex10`): the
    same picture on all four discs;
  - the one of the **levels' texts**, part of the page of the levels'
    own sprites (`tex31` of a level's texture file): one picture on the
    English disc, another, with more signs, on the three others, which
    agree.
- Which glyph a character is, is in each build's code: the reference
  knows letters and the apostrophe; each language's build maps its own
  accents to its own cells.

How it is to be built:

1. For each language's build, run its text functions in the interpreter
   on every character and record what they draw: this says, for each of
   its cells beyond the common ones, which sign it is.
2. Put the atlas of each typeface together: the common cells as they
   are, then one cell for each sign any language has. Write it into the
   texture files in place of each disc's own page, when the game is
   built (for the interpreter: when a texture file is read).
3. One table from character to glyph, or to letter and accent, for the
   atlas; the project's own, the same for every language.
4. One native function for each of the reference's text functions,
   drawing through that table with the reference's own layout. With the
   English texts the pictures must be the reference's.
5. The Italian executable's functions go.

This is a modification (ADR 0004): with it off the reference draws its
own text from its own pages.

Added by the owner the same day, for the one function:

- **A text can hold line breaks** (`\n`). The function lays the lines out
  itself: each aligned as the text is, the block placed by its height and
  width where the one line would have been. That is how a text that a
  language's build split into two texts drawn at two heights becomes one
  text again, and how a long one is made to fit.
- **Nothing is placed by a number written for one language.** What stands
  beside a text (the main menu's arrows) is placed from the text's own
  extent, worked out when it is drawn. Done for the main menu's arrows
  (`menu-arrows-by-text`), with the reference's letter step; to be led
  through the one function's measuring when it exists.

## The large typeface done (2026-11-21)

- The atlas is one page with **each sign once, in a place of its own**,
  whatever disc it comes from: the 48 common cells, then the two arrows,
  `Ç ¿ ¡ Ø Æ Œ`, the grave and acute accents, an empty place, the
  circumflex, diaeresis, tilde and ring, and the hyphen
  (`typeface::PLACES`). Every language can use every sign. The places are
  chosen so that no sign whose picture reaches the bottom of its cell
  stands over another (a sprite is drawn from the very edge of its cell
  and the picture repeats); this is checked when the atlas is made.
- Which sign a disc's cell is, is asked of the disc's own executable in a
  machine of its own (`Machine::record_calls_to`); its text function is
  found through the pairing, by position among the functions that differ.
- The discs at hand are the reference, `TS2_LANGUAGE_DIR` and those of
  `TS2_OTHER_DISCS`. A sign no disc at hand has is an empty cell.
- **One timing, the reference's** (the owner, the same day): no code of
  another build runs, so nothing of a language's build can make a screen
  last longer. The opening movies are the same files on the four discs.
- The texts worded by the project are one modification,
  `menu-load-save-wording`; the earlier correction of the Italian text is
  gone with it.
