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

The choice is the owner's and is open.
