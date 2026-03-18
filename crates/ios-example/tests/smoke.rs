use ios_example::{example_create, example_destroy, example_tick, example_update_scroll};
use ios_renderer_backend::types::LayerCmd;

#[test]
fn initial_frame_produces_commands() {
    let handle = example_create();

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0u32;

    // SAFETY: handle is valid, cmds has 1024 entries, count is valid.
    unsafe { example_tick(handle, 0, cmds.as_mut_ptr(), &mut count) };
    assert!(count > 0, "Initial frame must produce commands");

    example_destroy(handle);
}

#[test]
fn scroll_update_without_generation_change_produces_no_diff() {
    let handle = example_create();

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0u32;

    // Initial frame — consume all creation commands.
    // SAFETY: handle is valid, cmds has 1024 entries, count is valid.
    unsafe { example_tick(handle, 0, cmds.as_mut_ptr(), &mut count) };
    assert!(count > 0);

    // Scroll without changing the layout tree generation.
    example_update_scroll(handle, 0.0, 50.0);

    let mut count2 = 0u32;
    // SAFETY: handle is valid, cmds has 1024 entries, count2 is valid.
    unsafe { example_tick(handle, 16_000_000, cmds.as_mut_ptr(), &mut count2) };
    assert_eq!(
        count2, 0,
        "Pure scroll update with unchanged generation should produce 0 commands"
    );

    example_destroy(handle);
}

#[test]
fn create_destroy_cycle() {
    for _ in 0..10 {
        let handle = example_create();
        example_destroy(handle);
    }
}
