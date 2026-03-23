/// WAT module that builds a flex container with 4 colored child divs.
///
/// Layout:
/// - Parent: `display:flex; width:300px; height:300px; background-color:wheat`
/// - 4 children: `width:50px; height:50px` with red, green, blue, orange backgrounds
let demoWat = """
(module
  (import "env" "__CreateElement" (func $create (param i32) (result i32)))
  (import "env" "__SetInlineStyle" (func $style (param i32 i32 i32) (result i32)))
  (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
  (import "env" "__Commit" (func $commit (result i32)))
  (memory (export "memory") 1)

  ;; String data laid out in linear memory.
  (data (i32.const 0)   "div\\00")
  (data (i32.const 16)  "display\\00")
  (data (i32.const 32)  "flex\\00")
  (data (i32.const 48)  "width\\00")
  (data (i32.const 64)  "300px\\00")
  (data (i32.const 80)  "height\\00")
  (data (i32.const 96)  "background-color\\00")
  (data (i32.const 128) "wheat\\00")
  (data (i32.const 144) "50px\\00")
  (data (i32.const 160) "red\\00")
  (data (i32.const 176) "green\\00")
  (data (i32.const 192) "blue\\00")
  (data (i32.const 208) "orange\\00")

  (func (export "run") (result i32)
    (local $parent i32)
    (local $child i32)

    ;; Create parent div
    (local.set $parent (call $create (i32.const 0)))

    ;; Style parent: display:flex
    (drop (call $style (local.get $parent) (i32.const 16) (i32.const 32)))
    ;; width: 300px
    (drop (call $style (local.get $parent) (i32.const 48) (i32.const 64)))
    ;; height: 300px
    (drop (call $style (local.get $parent) (i32.const 80) (i32.const 64)))
    ;; background-color: wheat
    (drop (call $style (local.get $parent) (i32.const 96) (i32.const 128)))

    ;; Append parent to document root (node 0)
    (drop (call $append (i32.const 0) (local.get $parent)))

    ;; Child 1: red
    (local.set $child (call $create (i32.const 0)))
    (drop (call $style (local.get $child) (i32.const 48) (i32.const 144)))
    (drop (call $style (local.get $child) (i32.const 80) (i32.const 144)))
    (drop (call $style (local.get $child) (i32.const 96) (i32.const 160)))
    (drop (call $append (local.get $parent) (local.get $child)))

    ;; Child 2: green
    (local.set $child (call $create (i32.const 0)))
    (drop (call $style (local.get $child) (i32.const 48) (i32.const 144)))
    (drop (call $style (local.get $child) (i32.const 80) (i32.const 144)))
    (drop (call $style (local.get $child) (i32.const 96) (i32.const 176)))
    (drop (call $append (local.get $parent) (local.get $child)))

    ;; Child 3: blue
    (local.set $child (call $create (i32.const 0)))
    (drop (call $style (local.get $child) (i32.const 48) (i32.const 144)))
    (drop (call $style (local.get $child) (i32.const 80) (i32.const 144)))
    (drop (call $style (local.get $child) (i32.const 96) (i32.const 192)))
    (drop (call $append (local.get $parent) (local.get $child)))

    ;; Child 4: orange
    (local.set $child (call $create (i32.const 0)))
    (drop (call $style (local.get $child) (i32.const 48) (i32.const 144)))
    (drop (call $style (local.get $child) (i32.const 80) (i32.const 144)))
    (drop (call $style (local.get $child) (i32.const 96) (i32.const 208)))
    (drop (call $append (local.get $parent) (local.get $child)))

    ;; Commit triggers style resolution + layout
    (drop (call $commit))

    (i32.const 0)
  )
)
"""
