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

## Still to build (in this order)

- The settings in the game's own screens: bindings of mouse and joypad in
  the controls screen, a third camera in the pause menu beside "active" and
  "passive", sensitivity and inverted axes; kept in the project's settings
  file (ADR 0016), not in the game's saved block.
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
  (4681 counts a turn), lowered once on the owner's word; it wants a
  setting.
