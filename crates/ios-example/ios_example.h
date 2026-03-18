#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include "ios_renderer_backend.h"


/**
 * Create a renderer instance pre-loaded with the sample layout tree.
 *
 * Returns an opaque handle that must be passed to [`example_destroy`]
 * when no longer needed.
 */
uint64_t example_create(void);

/**
 * Run one frame of the rendering pipeline.
 *
 * Writes [`LayerCmd`] values into `out_cmds` and sets `*out_count` to
 * the number of commands produced.
 *
 * # Safety
 *
 * - `handle` must be a valid handle from [`example_create`].
 * - `out_cmds` must point to an array of at least 1024 [`LayerCmd`] values.
 * - `out_count` must point to a valid `u32`.
 */
void example_tick(uint64_t handle, uint64_t timestamp_ns, LayerCmd *out_cmds, uint32_t *out_count);

/**
 * Update the scroll offset for the main scroll container.
 *
 * Called from `UIScrollViewDelegate.scrollViewDidScroll` in Swift.
 */
void example_update_scroll(uint64_t handle, float offset_x, float offset_y);

/**
 * Destroy a renderer instance previously created with [`example_create`].
 *
 * Passing `0` is a no-op.
 */
void example_destroy(uint64_t handle);
