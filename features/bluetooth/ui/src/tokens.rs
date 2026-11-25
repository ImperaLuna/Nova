use gpui::{Pixels, px};

macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*
    };
    (@type px) => { Pixels };
    (@type ratio) => { f32 };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
}

tokens! {
    PAIRING_CELL_WIDTH: px = 28.0;
    PAIRING_CELL_HEIGHT: px = 36.0;
    PAIRING_CELL_GAP: px = 4.0;
    PAIRING_FIELD_HEIGHT: px = 34.0;
    PAIRING_TINT_OPACITY: ratio = 0.10;
    ROUND_BUTTON: px = 26.0;
    POPUP_WIDTH: px = 380.0;
    POPUP_RADIUS: px = 20.0;
}
