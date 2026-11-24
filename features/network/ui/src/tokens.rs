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
    TRAFFIC_GRAPH_HEIGHT: px = 54.0;
    LEGEND_DOT: px = 7.0;
    WIFI_ARC_FADED_OPACITY: ratio = 0.3;
    MORE_SIZE: px = 24.0;
    MENU_WIDTH: px = 170.0;
    PASSWORD_HEIGHT: px = 34.0;
}
