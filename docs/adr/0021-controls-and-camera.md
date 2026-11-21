# ADR 0021: Controls and camera — the mouse among the actions, a camera turned by it

Status: accepted (2026-11-21), first part built

## Context

The owner wants the game played as third-person games are played now: the
mouse turns a camera that goes around the character, its buttons attack and
aim, a joypad works, and all of it can be set in the game's options. The
original's controls stay available, and with the modifications off the game
is the original (ADR 0004).

What the game does was read from its code first (`docs/CODE-DISCOVERY.md`,
"Controls" and "Camera"):

- One function a frame (`0x414AF0`) reads the keyboard and the first
  joystick and leaves **a mask of sixteen actions** (`0x529B00`), with an
  analogue pair for walking. Everything else reads that mask and works out
  by itself when an action went down. Its table of bindings holds keys and
  joystick buttons; the game takes no notice of the mouse.
- The camera that follows the character (`0x4045E0`) **already goes around
  it on a sphere**: an angle sideways, an elevation, a distance. The
  function first decides those (the "active" camera turns behind the
  character; the "passive" one waits for the camera's two actions; both
  swing away from walls and come back to one elevation), and its tail
  places the camera from them, pushes it out of walls and turns it to the
  character.
- The directions the character walks in are already taken from the
  camera's place: not from where the character faces.
- The visor (first person), the cameras the levels script and the pause
  take the camera over in the function that calls the follower, not in it.

## Decision

1. **The mouse joins the actions where the game has just made them**
   (`mouse-controls`): at the `ret` of `0x414AF0` the actions bound to the
   mouse's buttons are added to the mask. Nothing is made up further back:
   no key presses, no device the game did not ask for. The game's own
   reading of keyboard and joystick is untouched, and so is its table.
2. **A binding is "the action while held" or "the visor while held"**. On
   a key the game's visor is a switch; a mouse button holds it, as aiming
   is held in a first-person game: the action is given when the button and
   the view disagree, and only a visor the button put on is put away when
   the button is let go.
   Defaults: left fires, middle spins, right holds the visor.
3. **The orbit camera replaces the decision of the two angles and nothing
   else** (`orbit-camera`): the stretch `0x4046CE`–`0x4053B1` of the
   follower. The angles are the player's (the mouse's movement of the
   frame, and the camera's two actions as the passive camera takes them);
   the height looked at follows the character as in the original; one cast
   of the game's own collision from the character's head to the camera's
   place brings the camera nearer, as the original does; and the game's
   tail places the camera, told to put it exactly (its "snap" marks) and
   not ease it. The visor, the scripted cameras and the pause are reached
   as before, since they are chosen outside this function.
4. **The mouse goes by the clock of recorded input** (ADR 0017): what it
   moved between two reads of the keyboard is that frame's movement
   (`Input::frame_motion`), and recordings carry it (`move X Y WHEEL`) and
   the buttons (`press MOUSE_RIGHT`). A session with the mouse replays as
   one with the keyboard does.
5. **In the visor the mouse aims** (part of `orbit-camera`): where the
   visor's function is about to turn the aim by its two rates
   (`0x403FC1`), the mouse's movement of the frame turns it first, straight
   and not through the rates: sideways it turns the character, as the
   visor does, and up and down it moves the aim's elevation within the
   visor's own limits. The direction actions go on working beside it.
6. **The window holds the mouse while the game has it**: a press on the
   picture takes it (pointer hidden and held), F10 or leaving the window
   gives it back. The game is given the mouse's own movement, not the
   pointer's.

7. **W, A, S and D walk, beside the arrows** (`walking-keys`, decided by
   the owner): added to the mask one instruction before the mouse's
   buttons, from the keyboard as the function has just read it, with the
   analogue pair at a whole push as the function's own keys leave it.
8. **A joypad is the joystick the game already asks for.** The game
   enumerates joysticks, reads the first through DirectInput and has its
   own bindings for ten buttons and its own analogue walking; all the
   runtime adds is the device (`Input::pad`: six axes, sixteen buttons, a
   direction pad; a polled device with the second device interface, axes
   from 0 to 65535). This works with every modification off. On top of it
   `orbit-camera` reads the pad's second stick: past its rest it turns the
   camera, and the visor's aim, in proportion.
   Recordings carry the pad (`0 plug PAD`, `axis X -20000`, `press PAD_1`,
   `hat 2`), read 0 being before the game starts, when it looks for
   joysticks.

9. **The pause menu lists three cameras.** Its camera page is a list of
   the game's two; three short stretches make it a list of three: the
   number of entries the page has (`0x49F518`), the choice
   (`0x49F6C5`–`0x49F6FC`) and the drawing (`0x4A0132`–`0x4A0179`), with
   the game's own text function and shades. The third camera's name is the
   project's, in the game's language. Which of the three is chosen is kept
   in the runtime, beside the game's own bit for its two; with one of the
   game's cameras chosen the stretch of the follower runs as the game's
   code.

10. **The options have a fifth entry, "mouse and camera"** (part of
    `mouse-controls`; `crates/runtime/src/options.rs`). Not rows of the
    controls screen: each of those captures one control into the game's
    saved table, which has no room. The options screen is one function
    with its own loop (`0x4371B0`); five short stretches of it are
    replaced, each standing for the instructions it takes the place of:
    - `0x437407`–`0x437410`, the three instructions that keep down from
      going past the fourth entry: it stops at the fifth. The pointers
      need nothing: the word after the table of the four heights
      (`0x4F6BE4`) holds 145, the fifth's height at the same step;
    - `0x43768C`, the first instruction after the four entries are drawn:
      the fifth is drawn with the same function, at 145 + 14;
    - `0x437374`, the first instruction of what the loop sets aside on
      entering a page: the page takes its copy of the settings;
    - `0x43773C`, the first instruction of the loop's search for the page
      the pointers are at: at the fifth entry a frame of the page is done
      and the loop goes on as after any page (`0x4376AE`);
    - `0x4375BB`, the first instruction of what the loop does when a page
      is accepted: the copy becomes the settings and they are written
      down. Cancel needs nothing: the copy is dropped.

    The page follows the game's graphics page (`0x49CAA0`): the title, a
    row every 25 units from 0x4B, the row chosen blinking, the two arrows
    of sheet 0x38 going after it, that page's own two lines of help, its
    delay of 15 time steps between directions and its sounds. Its rows:
    the mouse's sensitivity (1 to 10; a step is 56 of `Controls::turn`,
    both axes, and the fourth is the 224 the mouse had), the vertical
    axis (normal or inverted), the right button (holding the visor, or
    spinning; the middle button spins either way). The rows are drawn
    with the small typeface's function that places a text (`0x49B3B0`),
    centred: the one that centres on the screen (`0x49B2A0`) has no
    digits and draws 1 to 4 as the buttons' icons. Accepting sets only
    what a row was moved on.
    The pointers of `menu-arrows-by-text` take the fifth entry's width
    into account.
11. **The settings are kept between sessions** in `retold.ini` in the
    state directory, beside the game's saves, not in the game's saved
    block. It is read when the machine's system is made (`Os::new`), and
    written when the page is accepted and when a camera is chosen in the
    pause menu. No file means the defaults. Lines of `key=value`; `#`
    starts a comment; a line that is not a known key with one of its
    values is passed over and leaves that setting as it is:

    ```ini
    mouse_sensitivity=4      # 1 to 10
    vertical_inverted=no     # yes, no
    left_button=fire         # nothing, visor (while held), fire, spin
    right_button=visor
    middle_button=spin
    fourth_button=nothing
    camera=orbit             # orbit, game (one of the game's two)
    ```

    Which of the game's two cameras is the game's to remember (its bit is
    saved with the game). `boot` prints the settings it read.

## Still to build

- The joypad on Linux: the `gilrs` crate wants the system's libudev
  headers there, which this project does not ask to be installed. On
  Windows the window reads the first pad found when the game starts
  (`crates/cli/src/pad.rs`) — written and type-checked, never run.

## Consequences and what is not verified

- With `mouse-controls` on and the mouse untouched, a run is the original's
  to the instruction (the replaced `ret` is counted as the instruction it
  is): the recordings of `tools/check.sh` are unchanged.
- `orbit-camera` changes what a recording of the keyboard does, since the
  directions follow the camera: older recordings are replayed without it.
- The orbit camera does not do what the original's angle decision also did:
  the hint points and turns the level scripts give the camera, the states
  in which the original forces the camera behind the character (what each
  is, is `UNVERIFIED`), and the swinging away from walls. Where a level
  relies on one of them the view differs; none has been met yet.
- The elevation is kept between -0x280 and 0x380 (the original's own stays
  within -0x200 and 0x300); steeper views are `UNVERIFIED` against the
  game's culling.
- Not run on Windows or with a real mouse by the author: the window's part
  (holding the pointer, relative movement) is the owner's to try.
- The game's joystick code has been run with a recorded pad (walking with
  the stick, jumping with the first button); the range of an axis that was
  given none, 0 to 65535, and all ones for a direction pad that is not
  there are DirectInput's as remembered, `UNVERIFIED` against a reference.
- The mouse turns the camera by seven eighths of a unit of angle a count
  (4681 counts a turn) unless the options say otherwise: from a quarter
  of that to two and a half times it.
- With `mouse-controls` on, the list of options draws one text more, so a
  run that opens the options is no longer the original's to the
  instruction: the outcomes `tools/check.sh` has for `options` and
  `options-list` are to be recorded again. With it off the options are
  the original's (the `options` recording gives the same instructions
  and the same frames as before).
- A recording replays from the state it was made from, and the settings
  file is part of that state: one that sets something leaves it for the
  next run from the same directory.
- The page was run in English, with the one typeface and with the
  original's text functions. The wordings of the other languages were
  not seen drawn, and neither the page in the compiled game
  (`--check-blocks`): `UNVERIFIED`.
