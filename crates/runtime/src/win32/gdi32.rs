//! GDI32: drawing objects.

use super::functions;

/// Handles of stock objects: tag in the top byte, stock index below.
const STOCK_TAG: u32 = 0x0180_0000;

functions! { "GDI32.dll";
    GetStockObject(1) => |_, a| if a[0] <= 19 { STOCK_TAG | a[0] } else { 0 },
}
