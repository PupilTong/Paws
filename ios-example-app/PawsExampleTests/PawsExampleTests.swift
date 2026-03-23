import XCTest
import PawsRenderer
@testable import PawsExample

final class PawsExampleTests: XCTestCase {

    func testRendererCreation() {
        let renderer = PawsRendererInstance()
        // Verify basic creation doesn't crash.
        let id = renderer.createElement("div")
        XCTAssertGreaterThan(id, 0)
    }

    func testWatExecution() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        // runWat preconditions on success — this verifies the WAT compiles and runs.
        view.renderer.runWat(demoWat, functionName: "run")
    }

    func testCommitProducesLayers() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        view.renderer.runWat(demoWat, functionName: "run")

        // After commit the root view should have child content.
        // The root LayoutBox creates a UIView (subview of rendererView).
        // That UIView's layer should contain CALayer sublayers for the 4 child divs.
        let expectation = expectation(description: "layout applied")
        DispatchQueue.main.async {
            // The renderer view should have at least one subview (the root div).
            XCTAssertFalse(
                view.subviews.isEmpty,
                "PawsRendererView should have a subview after commit"
            )

            if let rootDiv = view.subviews.first {
                // The root div (UIView) should have sublayers for the 4 child divs.
                let sublayers = rootDiv.layer.sublayers ?? []
                XCTAssertEqual(
                    sublayers.count, 4,
                    "Root div should have 4 CALayer sublayers for child divs"
                )
            }
            expectation.fulfill()
        }
        wait(for: [expectation], timeout: 2.0)
    }

    func testLayerFrames() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        view.renderer.runWat(demoWat, functionName: "run")

        let expectation = expectation(description: "layer frames")
        DispatchQueue.main.async {
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
        wait(for: [expectation], timeout: 2.0)
    }

    func testLayerBackgroundColors() {
        let view = PawsRendererView(
            baseURL: "about:blank",
            frame: CGRect(x: 0, y: 0, width: 300, height: 300)
        )
        view.renderer.runWat(demoWat, functionName: "run")

        let expectation = expectation(description: "layer colors")
        DispatchQueue.main.async {
            guard let rootDiv = view.subviews.first,
                  let sublayers = rootDiv.layer.sublayers,
                  sublayers.count == 4 else {
                XCTFail("Expected 4 sublayers")
                expectation.fulfill()
                return
            }

            for (i, layer) in sublayers.enumerated() {
                XCTAssertNotNil(
                    layer.backgroundColor,
                    "Child \(i) should have a background color"
                )
            }
            expectation.fulfill()
        }
        wait(for: [expectation], timeout: 2.0)
    }
}
