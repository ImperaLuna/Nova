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
    RING_SIZE: px = 52.0;
    RING_STROKE: px = 4.0;
    GRAPH_HEIGHT: px = 72.0;
    GRAPH_AREA_OPACITY: ratio = 0.22;
    LEGEND_DOT: px = 7.0;
    PROCESS_ROW_HEIGHT: px = 30.0;
    PROCESS_NUMBER_WIDTH: px = 64.0;
    END_BUTTON_WIDTH: px = 26.0;
    END_BUTTON_HEIGHT: px = 24.0;
    END_ICON_SIZE: px = 13.0;
    END_HOVER_OPACITY: ratio = 0.22;
    CONFIRM_OPACITY: ratio = 0.14;
    SEARCH_HEIGHT: px = 34.0;
}
