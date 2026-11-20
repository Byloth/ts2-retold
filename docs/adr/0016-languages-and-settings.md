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
