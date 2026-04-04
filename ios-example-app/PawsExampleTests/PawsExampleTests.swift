import XCTest
import PawsRenderer

/// WAT module that creates 4 colored divs inside a flex container and commits.
private let demoWat = """
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\\00")
  (data (i32.const 16) "display\\00")
  (data (i32.const 32) "flex\\00")
  (data (i32.const 48) "width\\00")
  (data (i32.const 64) "50px\\00")
  (data (i32.const 80) "height\\00")
  (data (i32.const 96) "background-color\\00")
  (data (i32.const 128) "red\\00")
  (data (i32.const 144) "green\\00")
  (data (i32.const 160) "blue\\00")
  (data (i32.const 176) "orange\\00")
  (func (export "run") (result i32)
    (local $root i32)
    (local $c i32)
    ;; Root flex container
    (local.set $root (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $root)))
    (drop (call $style (local.get $root) (i32.const 16) (i32.const 32)))
    ;; Child 1 - red
    (local.set $c (call $create (i32.const 0)))
    (drop (call $append (local.get $root) (local.get $c)))
    (drop (call $style (local.get $c) (i32.const 48) (i32.const 64)))
    (drop (call $style (local.get $c) (i32.const 80) (i32.const 64)))
    (drop (call $style (local.get $c) (i32.const 96) (i32.const 128)))
    ;; Child 2 - green
    (local.set $c (call $create (i32.const 0)))
    (drop (call $append (local.get $root) (local.get $c)))
    (drop (call $style (local.get $c) (i32.const 48) (i32.const 64)))
    (drop (call $style (local.get $c) (i32.const 80) (i32.const 64)))
    (drop (call $style (local.get $c) (i32.const 96) (i32.const 144)))
    ;; Child 3 - blue
    (local.set $c (call $create (i32.const 0)))
    (drop (call $append (local.get $root) (local.get $c)))
    (drop (call $style (local.get $c) (i32.const 48) (i32.const 64)))
    (drop (call $style (local.get $c) (i32.const 80) (i32.const 64)))
    (drop (call $style (local.get $c) (i32.const 96) (i32.const 160)))
    ;; Child 4 - orange
    (local.set $c (call $create (i32.const 0)))
    (drop (call $append (local.get $root) (local.get $c)))
    (drop (call $style (local.get $c) (i32.const 48) (i32.const 64)))
    (drop (call $style (local.get $c) (i32.const 80) (i32.const 64)))
    (drop (call $style (local.get $c) (i32.const 96) (i32.const 176)))
    ;; Commit
    (drop (call $commit))
    (i32.const 0)
  )
)
"""

/// WAT module that creates a div with a text node and commits.
private let textWat = """
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__create_text_node" (func $text (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\\00")
  (data (i32.const 16) "width\\00")
  (data (i32.const 32) "200px\\00")
  (data (i32.const 48) "Hello Paws\\00")
  (func (export "run") (result i32)
    (local $div i32)
    (local $txt i32)
    (local.set $div (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $div)))
    (drop (call $style (local.get $div) (i32.const 16) (i32.const 32)))
    (local.set $txt (call $text (i32.const 48)))
    (drop (call $append (local.get $div) (local.get $txt)))
    (drop (call $commit))
    (i32.const 0)
  )
)
"""

final class PawsExampleTests: XCTestCase {

    func testRendererCreation() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        // Verify basic creation doesn't crash.
        XCTAssertNotNil(view.renderer)
    }

    func testWatExecution() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        let expectation = expectation(description: "ops executed")
        view.renderer.executor.onExecute = {
            expectation.fulfill()
        }

        view.renderer.postRunWat(demoWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }

    func testCommitProducesLayers() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        let expectation = expectation(description: "layout applied")
        view.renderer.executor.onExecute = {
            XCTAssertFalse(
                view.subviews.isEmpty,
                "PawsRendererView should have a subview after commit"
            )

            if let rootDiv = view.subviews.first {
                let sublayers = rootDiv.layer.sublayers ?? []
                XCTAssertEqual(
                    sublayers.count, 4,
                    "Root div should have 4 CALayer sublayers for child divs"
                )
            }
            expectation.fulfill()
        }

        view.renderer.postRunWat(demoWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }

    func testLayerFrames() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        let expectation = expectation(description: "layer frames")
        view.renderer.executor.onExecute = {
            guard let rootDiv = view.subviews.first,
                  let sublayers = rootDiv.layer.sublayers,
                  sublayers.count == 4 else {
                XCTFail("Expected 4 sublayers")
                expectation.fulfill()
                return
            }

            for (i, layer) in sublayers.enumerated() {
                XCTAssertEqual(
                    layer.frame.width, 50,
                    accuracy: 0.1,
                    "Child \(i) width should be 50"
                )
                XCTAssertEqual(
                    layer.frame.height, 50,
                    accuracy: 0.1,
                    "Child \(i) height should be 50"
                )
            }
            expectation.fulfill()
        }

        view.renderer.postRunWat(demoWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }

    func testLayerBackgroundColors() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        let expectation = expectation(description: "layer colors")
        view.renderer.executor.onExecute = {
            guard let rootDiv = view.subviews.first,
                  let sublayers = rootDiv.layer.sublayers,
                  sublayers.count == 4 else {
                XCTFail("Expected 4 sublayers")
                expectation.fulfill()
                return
            }

            let expectedColors: [(r: CGFloat, g: CGFloat, b: CGFloat)] = [
                (1.0, 0.0, 0.0),         // red
                (0.0, 128.0/255.0, 0.0),  // green (#008000)
                (0.0, 0.0, 1.0),          // blue
                (1.0, 165.0/255.0, 0.0),  // orange (#FFA500)
            ]

            for (i, layer) in sublayers.enumerated() {
                guard let bgColor = layer.backgroundColor,
                      let components = bgColor.components,
                      components.count >= 3 else {
                    XCTFail("Child \(i) should have a background color with RGB components")
                    continue
                }

                let expected = expectedColors[i]
                let tolerance: CGFloat = 0.02
                XCTAssertEqual(
                    components[0], expected.r, accuracy: tolerance,
                    "Child \(i) red component mismatch"
                )
                XCTAssertEqual(
                    components[1], expected.g, accuracy: tolerance,
                    "Child \(i) green component mismatch"
                )
                XCTAssertEqual(
                    components[2], expected.b, accuracy: tolerance,
                    "Child \(i) blue component mismatch"
                )
            }
            expectation.fulfill()
        }

        view.renderer.postRunWat(demoWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }

    // MARK: - Text rendering tests

    func testTextNodeCreatesTextLayer() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        let expectation = expectation(description: "text layer created")
        view.renderer.executor.onExecute = {
            guard let rootDiv = view.subviews.first else {
                XCTFail("Expected root div subview")
                expectation.fulfill()
                return
            }

            let sublayers = rootDiv.layer.sublayers ?? []
            let textLayers = sublayers.compactMap { $0 as? CATextLayer }
            XCTAssertEqual(
                textLayers.count, 1,
                "Should have 1 CATextLayer for the text node"
            )
            expectation.fulfill()
        }

        view.renderer.postRunWat(textWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }

    func testTextLayerHasContent() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        let expectation = expectation(description: "text content set")
        view.renderer.executor.onExecute = {
            guard let rootDiv = view.subviews.first,
                  let sublayers = rootDiv.layer.sublayers else {
                XCTFail("Expected root div with sublayers")
                expectation.fulfill()
                return
            }

            let textLayers = sublayers.compactMap { $0 as? CATextLayer }
            guard let textLayer = textLayers.first else {
                XCTFail("Expected a CATextLayer")
                expectation.fulfill()
                return
            }

            XCTAssertEqual(
                textLayer.string as? String, "Hello Paws",
                "CATextLayer should contain the text node content"
            )
            expectation.fulfill()
        }

        view.renderer.postRunWat(textWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }

    func testTextLayerHasNonZeroFrame() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        let expectation = expectation(description: "text frame")
        view.renderer.executor.onExecute = {
            guard let rootDiv = view.subviews.first,
                  let sublayers = rootDiv.layer.sublayers else {
                XCTFail("Expected root div with sublayers")
                expectation.fulfill()
                return
            }

            let textLayers = sublayers.compactMap { $0 as? CATextLayer }
            guard let textLayer = textLayers.first else {
                XCTFail("Expected a CATextLayer")
                expectation.fulfill()
                return
            }

            XCTAssertGreaterThan(
                textLayer.frame.width, 0,
                "Text layer should have positive width"
            )
            XCTAssertGreaterThan(
                textLayer.frame.height, 0,
                "Text layer should have positive height"
            )
            expectation.fulfill()
        }

        view.renderer.postRunWat(textWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }

    // MARK: - Screenshot / pixel comparison tests

    func testScreenshotColoredDivs() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 200, height: 50)
        )
        let expectation = expectation(description: "screenshot")
        view.renderer.executor.onExecute = {
            // Capture the view hierarchy as a bitmap.
            let renderer = UIGraphicsImageRenderer(size: view.bounds.size)
            let image = renderer.image { ctx in
                view.layer.render(in: ctx.cgContext)
            }
            guard let cgImage = image.cgImage,
                  let dataProvider = cgImage.dataProvider,
                  let pixelData = dataProvider.data else {
                XCTFail("Failed to capture screenshot")
                expectation.fulfill()
                return
            }

            let data = pixelData as Data
            let bytesPerPixel = cgImage.bitsPerPixel / 8
            let bytesPerRow = cgImage.bytesPerRow

            /// Reads the RGBA pixel at (x, y) from the bitmap.
            func pixel(x: Int, y: Int) -> (r: UInt8, g: UInt8, b: UInt8, a: UInt8) {
                let offset = y * bytesPerRow + x * bytesPerPixel
                return (data[offset], data[offset+1], data[offset+2], data[offset+3])
            }

            // The demoWat creates 4 children of 50x50 in a flex row.
            // Child 0 (red) occupies x=0..50, child 1 (green) x=50..100, etc.
            let tolerance: UInt8 = 5

            // Sample red child at (25, 25)
            let red = pixel(x: 25, y: 25)
            XCTAssertGreaterThan(red.r, 200, "Red child should have high red")
            XCTAssertLessThan(red.g, tolerance, "Red child should have low green")
            XCTAssertLessThan(red.b, tolerance, "Red child should have low blue")

            // Sample green child at (75, 25) — CSS green is #008000
            let green = pixel(x: 75, y: 25)
            XCTAssertLessThan(green.r, tolerance, "Green child should have low red")
            XCTAssertGreaterThan(green.g, 100, "Green child should have positive green")
            XCTAssertLessThan(green.b, tolerance, "Green child should have low blue")

            // Sample blue child at (125, 25)
            let blue = pixel(x: 125, y: 25)
            XCTAssertLessThan(blue.r, tolerance, "Blue child should have low red")
            XCTAssertLessThan(blue.g, tolerance, "Blue child should have low green")
            XCTAssertGreaterThan(blue.b, 200, "Blue child should have high blue")

            // Sample orange child at (175, 25) — CSS orange is #FFA500
            let orange = pixel(x: 175, y: 25)
            XCTAssertGreaterThan(orange.r, 200, "Orange child should have high red")
            XCTAssertGreaterThan(orange.g, 140, "Orange child should have medium green")
            XCTAssertLessThan(orange.b, tolerance, "Orange child should have low blue")

            expectation.fulfill()
        }

        view.renderer.postRunWat(demoWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }

    func testScreenshotTextNodeHasContent() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 200, height: 50)
        )
        let expectation = expectation(description: "text screenshot")
        view.renderer.executor.onExecute = {
            // Capture the rendered view to verify text produces non-transparent pixels.
            let renderer = UIGraphicsImageRenderer(size: view.bounds.size)
            let image = renderer.image { ctx in
                view.layer.render(in: ctx.cgContext)
            }
            guard let cgImage = image.cgImage,
                  let dataProvider = cgImage.dataProvider,
                  let pixelData = dataProvider.data else {
                XCTFail("Failed to capture screenshot")
                expectation.fulfill()
                return
            }

            let data = pixelData as Data
            let bytesPerPixel = cgImage.bitsPerPixel / 8
            let bytesPerRow = cgImage.bytesPerRow
            let width = cgImage.width
            let height = cgImage.height

            // Count non-transparent pixels — text should produce some ink.
            var nonTransparent = 0
            for y in 0..<height {
                for x in 0..<width {
                    let offset = y * bytesPerRow + x * bytesPerPixel
                    let alpha = data[offset + 3]
                    if alpha > 0 {
                        nonTransparent += 1
                    }
                }
            }

            XCTAssertGreaterThan(
                nonTransparent, 0,
                "Text rendering should produce non-transparent pixels"
            )
            expectation.fulfill()
        }

        view.renderer.postRunWat(textWat, functionName: "run")
        wait(for: [expectation], timeout: 5.0)
    }
}
