#ifndef IOS_RENDERER_BACKEND_H
#define IOS_RENDERER_BACKEND_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * The kind of native view/layer to create on the Swift side.
 */
typedef enum LayerKind {
  View,
  ScrollView,
  MetalLayer,
} LayerKind;

/**
 * Describes whether a child is parented to a plain `CALayer` or a
 * `UIScrollView`'s content layer.
 */
typedef enum ParentKind {
  Layer,
  ScrollContent,
} ParentKind;

/**
 * A node in the fully-computed layout tree produced by the engine.
 *
 * The tree is owned by the caller and borrowed immutably during a single
 * `render_frame` pass.
 */
typedef struct LayoutNode LayoutNode;

/**
 * Unique identifier for a composited layer.
 */
typedef uint64_t LayerId;

/**
 * Axis-aligned rectangle in screen-space coordinates.
 */
typedef struct RBRect {
  float x;
  float y;
  float width;
  float height;
} RBRect;

/**
 * RGBA color with premultiplied alpha, components in `[0.0, 1.0]`.
 */
typedef struct RBColor {
  float r;
  float g;
  float b;
  float a;
} RBColor;

/**
 * Column-major 4×4 affine transform (matches `CATransform3D` layout).
 */
typedef struct Transform3D {
  float m[16];
} Transform3D;

/**
 * Flat, FFI-safe bundle of visual properties for a single `CALayer`.
 *
 * `Option<Transform3D>` and `Option<Rect>` are encoded as a `bool` flag
 * (`has_transform`, `has_clip`) paired with the value so the struct stays
 * `#[repr(C)]`.
 */
typedef struct LayerProps {
  struct RBRect frame;
  float opacity;
  struct RBColor background;
  float border_radius;
  bool has_transform;
  struct Transform3D transform;
  bool has_clip;
  struct RBRect clip;
} LayerProps;

/**
 * Two-dimensional size.
 */
typedef struct RBSize {
  float width;
  float height;
} RBSize;

/**
 * A single command that mutates the native layer tree on the Swift side.
 *
 * Commands are emitted in painter's order (parents before children) and
 * written into a caller-allocated buffer via `rb_render_frame`.
 */
typedef enum LayerCmd_Tag {
  CreateLayer,
  UpdateLayer,
  RemoveLayer,
  AttachScroll,
  SetZOrder,
  ReparentLayer,
} LayerCmd_Tag;

typedef struct CreateLayer_Body {
  LayerId id;
  enum LayerKind kind;
} CreateLayer_Body;

typedef struct UpdateLayer_Body {
  LayerId id;
  struct LayerProps props;
} UpdateLayer_Body;

typedef struct RemoveLayer_Body {
  LayerId id;
} RemoveLayer_Body;

typedef struct AttachScroll_Body {
  LayerId id;
  LayerId parent_id;
  struct RBSize content_size;
} AttachScroll_Body;

typedef struct SetZOrder_Body {
  LayerId id;
  uint32_t index;
} SetZOrder_Body;

typedef struct ReparentLayer_Body {
  LayerId id;
  LayerId new_parent;
  enum ParentKind parent_type;
} ReparentLayer_Body;

typedef struct LayerCmd {
  LayerCmd_Tag tag;
  union {
    CreateLayer_Body create_layer;
    UpdateLayer_Body update_layer;
    RemoveLayer_Body remove_layer;
    AttachScroll_Body attach_scroll;
    SetZOrder_Body set_z_order;
    ReparentLayer_Body reparent_layer;
  };
} LayerCmd;

/**
 * Create a new renderer instance and return an opaque handle.
 *
 * `layer_pool_capacity` controls pre-allocation for the command buffer
 * and internal vec pools. A typical value is 1024.
 *
 * The returned handle must be passed to [`rb_destroy`] when no longer
 * needed.
 */
uint64_t rb_create(uint32_t layer_pool_capacity);

/**
 * Destroy a renderer instance previously created with [`rb_create`].
 *
 * Passing `0` is a no-op.
 */
void rb_destroy(uint64_t handle);

/**
 * Execute one frame of the rendering pipeline.
 *
 * Writes [`LayerCmd`] values into the caller-allocated buffer at
 * `out_cmds` (which must have room for at least `pool_capacity` entries)
 * and sets `*out_count` to the number of commands written.
 *
 * Called from the `CAMetalDisplayLink` callback on the main thread in
 * Swift.
 *
 * # Safety
 *
 * - `handle` must be a valid, non-zero handle from [`rb_create`].
 * - `out_cmds` must point to an array of at least `pool_capacity`
 *   [`LayerCmd`] values.
 * - `out_count` must point to a valid `u32`.
 */
void rb_render_frame(uint64_t handle,
                     uint64_t _timestamp_ns,
                     struct LayerCmd *out_cmds,
                     uint32_t *out_count);

/**
 * Update the scroll offset for a scroll container.
 *
 * Called from `UIScrollViewDelegate.scrollViewDidScroll` in Swift.
 * May be called concurrently with rayon pool reads (lock-free atomic
 * update).
 */
void rb_update_scroll_offset(uint64_t handle, uint64_t scroll_id, float offset_x, float offset_y);

/**
 * Submit a new layout tree for rendering.
 *
 * The tree rooted at `root` is deep-cloned (O(n) in node count).
 * Passing a null `root` clears the current layout.
 *
 * # Safety
 *
 * - `handle` must be a valid, non-zero handle from [`rb_create`].
 * - `root`, if non-null, must point to a valid [`LayoutNode`].
 */
void rb_submit_layout(uint64_t handle, const struct LayoutNode *root, uint32_t _node_count);

/**
 * Submit a built-in demo layout tree: a scrollable list of colored rows.
 *
 * Useful for verifying the Swift rendering pipeline without needing a
 * separate WASM module or Rust crate to construct layout trees.
 *
 * `viewport_w` / `viewport_h` are the screen dimensions in points.
 * `row_count` controls how many rows appear in the scrollable list.
 */
void rb_submit_demo_layout(uint64_t handle, float viewport_w, float viewport_h, uint32_t row_count);

/**
 * Change the command buffer capacity for future frames.
 */
void rb_set_pool_capacity(uint64_t handle, uint32_t capacity);

#endif /* IOS_RENDERER_BACKEND_H */
