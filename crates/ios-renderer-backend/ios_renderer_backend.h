#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum LayerKind {
  View,
  ScrollView,
  MetalLayer,
} LayerKind;

typedef enum ParentKind {
  Layer,
  ScrollView,
} ParentKind;

typedef struct LayoutNode LayoutNode;

typedef uint64_t LayerId;

typedef struct Rect {
  float x;
  float y;
  float width;
  float height;
} Rect;

typedef struct Color {
  float r;
  float g;
  float b;
  float a;
} Color;

typedef struct Transform3D {
  float m[16];
} Transform3D;

typedef struct LayerProps {
  struct Rect frame;
  float opacity;
  struct Color background;
  float border_radius;
  bool has_transform;
  struct Transform3D transform;
  bool has_clip;
  struct Rect clip;
} LayerProps;

typedef struct Size {
  float width;
  float height;
} Size;

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
  struct Size content_size;
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

uint64_t rb_create(uint32_t layer_pool_capacity);

void rb_destroy(uint64_t handle);

void rb_render_frame(uint64_t handle,
                     uint64_t _timestamp_ns,
                     struct LayerCmd *out_cmds,
                     uint32_t *out_count);

void rb_update_scroll_offset(uint64_t handle, uint64_t scroll_id, float offset_x, float offset_y);

void rb_submit_layout(uint64_t handle, const struct LayoutNode *root, uint32_t _node_count);

void rb_set_pool_capacity(uint64_t handle, uint32_t capacity);
