//! WINMM: the multimedia timer (and, later, wave file reading).

use super::functions;

functions! { "WINMM.dll";
    timeGetTime(0) => |m, _| m.os.now_ms,
}
