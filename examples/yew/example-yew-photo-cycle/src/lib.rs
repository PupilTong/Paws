//! Three photos cycling 1 -> 2 -> 3 -> 1 on click.
//!
//! Each photo is inlined at compile time via `inline_image!` and
//! registered with the host as a `blob:paws/<hex>` URL once at mount
//! (in the `use_state` initializer, which runs synchronously on the
//! first render). The component holds all three URLs and picks one
//! by index; clicks bump the index modulo 3.
//!
//! **Note on the click handler.** Paws' event system and hit-testing
//! are not yet wired end-to-end, so tapping the image will not fire
//! the `onclick` callback today. The handler body below is the
//! intended shape for when those subsystems land; until then the
//! example renders photo-1 statically after mount.
//!
//! Exercises the full path: `inline_image!` (compile-time read) ->
//! `create_object_url_with_raw_data` (host registration) ->
//! `<img src="blob:paws/...">` (DOM attribute) ->
//! `ResourceResolver` (renderer-side decode).
//!
//! The example deliberately keeps all three URLs alive for the
//! component's lifetime rather than revoking on each cycle. Once
//! events land we can switch to revoke-on-cycle to also exercise
//! `revoke_object_url`; for now keeping URLs alive is the simpler
//! shape and avoids the use_effect re-render dance.

use std::rc::Rc;

use rust_wasm_binding::{create_object_url_with_raw_data, inline_image, Element, NodeOps};
use yew::prelude::*;

const PHOTO_1: (&[u8], &str) = inline_image!("assets/photo-1.png");
const PHOTO_2: (&[u8], &str) = inline_image!("assets/photo-2.png");
const PHOTO_3: (&[u8], &str) = inline_image!("assets/photo-3.png");

/// Stylesheet for the example. Paws does not parse `style="..."`
/// attributes into inline styles (only the explicit `set_inline_style`
/// host call does), so sizing for the `<img>` goes through a
/// stylesheet instead. Yew's `html!` does not give us a hook to call
/// `set_inline_style` per-element either, so this is the canonical
/// shape for sized elements in a Yew tree.
const STYLESHEET: &str = ".paws-photo { width: 240px; height: 240px; }";

#[function_component]
fn PhotoCycle() -> Html {
    // Mint all three blob URLs once at mount. `use_state(|| ...)`'s
    // initializer runs synchronously on the first render, which
    // means the very first commit already has a valid `src` —
    // unlike `use_effect_with`, which fires after the first render
    // and would leave the initial frame with an empty `<img>`.
    let urls = use_state(|| {
        [PHOTO_1, PHOTO_2, PHOTO_3]
            .iter()
            .map(|(bytes, mime)| create_object_url_with_raw_data(bytes, mime))
            .collect::<Vec<String>>()
    });
    let index = use_state(|| 0usize);

    let onclick = {
        let index = index.clone();
        Callback::from(move |_: ()| index.set((*index + 1) % 3))
    };

    let src = urls[*index].clone();

    html! {
        <img src={src} alt="cycling photo" class="paws-photo" onclick={onclick} />
    }
}

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let root = match Element::new("div") {
            Ok(element) => Rc::new(element),
            Err(error_code) => return error_code,
        };

        if let Err(error_code) = rust_wasm_binding::append_element(0, root.id()) {
            return error_code;
        }

        if let Err(error_code) = rust_wasm_binding::add_stylesheet(STYLESHEET) {
            return error_code;
        }

        let _app = yew::Renderer::<PhotoCycle>::with_root(root).render();
        0
    }
}
