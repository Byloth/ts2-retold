//! OLE32: the COM library. Objects here are created by the runtime itself,
//! so initialisation has nothing to do.

use super::functions;

functions! { "ole32.dll";
    CoInitialize(1) => |_, _| 0, // S_OK
    CoUninitialize(0) => |_, _| 0,
}
