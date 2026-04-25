//! Three photos cycling 1 -> 2 -> 3 -> 1 on click.
//!
//! Each photo is inlined at compile time via `inline_image!` and
//! registered with the host as a `blob:paws/<hex>` URL the first time
//! it is needed. On each index change the previously-active URL is
//! revoked (mirroring `URL.revokeObjectURL`) so the host drops its
//! copy of the bytes; the next frame creates a fresh URL for the new
//! photo.
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
//! `ResourceResolver` (renderer-side decode) -> `revoke_object_url`
//! (cleanup).

use std::rc::Rc;

use rust_wasm_binding::{
    create_object_url_with_raw_data, inline_image, revoke_object_url, Element, NodeOps,
};
use yew::prelude::*;

const PHOTO_1: (&[u8], &str) = inline_image!("assets/photo-1.png");
const PHOTO_2: (&[u8], &str) = inline_image!("assets/photo-2.png");
const PHOTO_3: (&[u8], &str) = inline_image!("assets/photo-3.png");

fn photo_for(index: usize) -> (&'static [u8], &'static str) {
    match index % 3 {
        0 => PHOTO_1,
        1 => PHOTO_2,
        _ => PHOTO_3,
    }
}

#[function_component]
fn PhotoCycle() -> Html {
    let index = use_state(|| 0usize);
    // Holds the currently-displayed blob URL so we can revoke it when
    // the index changes. `UseStateHandle<Option<String>>` gives us
    // cheap clones inside the effect + handler closures.
    let url = use_state(|| None::<String>);

    // On every index change: revoke the previous URL (if any), mint a
    // new one from the matching photo, and remember it. `use_effect_with`
    // re-runs whenever `index` changes; the previous run's return-value
    // closure is not used here because the revocation sequencing is
    // cleaner as "revoke then create" inside the new run.
    {
        let url = url.clone();
        let index = *index;
        use_effect_with(index, move |_| {
            if let Some(old) = (*url).clone() {
                let _ = revoke_object_url(&old);
            }
            let (bytes, mime) = photo_for(index);
            let new_url = create_object_url_with_raw_data(bytes, mime);
            url.set(Some(new_url));
            || ()
        });
    }

    let onclick = {
        let index = index.clone();
        Callback::from(move |_: ()| index.set(*index + 1))
    };

    let src = (*url).clone().unwrap_or_default();

    html! {
        <img src={src} alt="cycling photo" onclick={onclick}
             style="width:240px;height:240px;" />
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

        let _app = yew::Renderer::<PhotoCycle>::with_root(root).render();
        0
    }
}
