import XCTest
import PawsRenderer
@testable import PawsExample

final class PawsExampleTests: XCTestCase {

    func testRendererCreation() {
        let view = UIView(frame: CGRect(x: 0, y: 0, width: 300, height: 300))
        let renderer = PawsRendererInstance(baseURL: "about:blank", rootView: view)
        // Verify basic creation doesn't crash.
        let id = renderer.createElement("div")
        XCTAssertGreaterThan(id, 0)
    }

    func testWatExecution() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        // Fulfill when the OpExecutor processes the op buffer.
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
            // The renderer view should have content after ops execute.
            // Root LayoutBox creates a UIView as a subview of rendererView.
            // That UIView's layer should contain CALayer sublayers for child divs.
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

            // CSS named colors: red=#FF0000, green=#008000, blue=#0000FF, orange=#FFA500
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
}
