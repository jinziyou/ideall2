import XCTest

final class IdeallSmokeTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
        XCUIDevice.shared.orientation = .portrait
    }

    override func tearDownWithError() throws {
        XCUIDevice.shared.orientation = .portrait
    }

    func testCreateNoteInputAndLifecycleRecovery() throws {
        let app = XCUIApplication()
        app.launchEnvironment["IDEALL_CRASH_DIAGNOSTICS"] = "1"
        app.launchEnvironment["IDEALL_UI_TESTING"] = "1"
        configureLaunch(
            app,
            smokeAction: 1,
            smokeText: "ideall iOS smoke body second line"
        )
        app.launch()

        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 20))
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        attachScreenshot(named: "01-launched", from: window)

        // GPUI paints its controls without a complete native accessibility
        // tree, and its embedded UIWindow is not a UIScene touch target under
        // XCTest. UI-test-only launch actions exercise the real GPUI business
        // paths while Android smoke covers physical touch hit-testing.
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let deviceWindow = springboard.windows.firstMatch
        XCTAssertTrue(deviceWindow.waitForExistence(timeout: 10))
        let deviceFrame = deviceWindow.frame
        assertFullScreen(window.frame, matches: deviceFrame)
        _ = try XCTUnwrap(
            waitForInput("正文", containing: "second line", in: app)
        )
        waitForAutosave()

        relaunch(app, smokeAction: 2, smokeText: "ideall iOS smoke title")
        _ = try XCTUnwrap(
            waitForInput("标题", containing: "ideall iOS smoke title", in: app)
        )
        waitForAutosave()

        relaunch(app, smokeAction: 3, smokeText: " third line")
        let resumedBodyInput = try XCTUnwrap(
            waitForInput("正文", containing: "third line", in: app)
        )
        XCTAssertTrue(
            (resumedBodyInput.value as? String)?.contains("second line") == true,
            "正文应在重启并切换焦点后恢复"
        )
        waitForAutosave()
        attachScreenshot(named: "02-edited-note", from: window)

        app.swipeUp()
        XCUIDevice.shared.orientation = .landscapeLeft
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        attachScreenshot(named: "03-landscape", from: window)

        XCUIDevice.shared.press(.home)
        XCTAssertTrue(springboard.wait(for: .runningForeground, timeout: 10))
        app.activate()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 15))

        XCUIDevice.shared.orientation = .portrait
        attachScreenshot(named: "04-resumed", from: app.windows.firstMatch)

        app.terminate()
        configureLaunch(app, smokeAction: nil)
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 20))
        attachScreenshot(named: "05-relaunched", from: app.windows.firstMatch)
    }

    private func attachScreenshot(named name: String, from element: XCUIElement) {
        let attachment = XCTAttachment(screenshot: element.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func waitForInput(
        _ label: String,
        containing text: String,
        in app: XCUIApplication
    ) -> XCUIElement? {
        let input = app.textViews[label]
        guard input.waitForExistence(timeout: 5) else {
            return nil
        }
        let valueContainsText = NSPredicate { object, _ in
            guard let element = object as? XCUIElement,
                  let value = element.value as? String
            else {
                return false
            }
            return value.contains(text)
        }
        let expectation = XCTNSPredicateExpectation(predicate: valueContainsText, object: input)
        guard XCTWaiter.wait(for: [expectation], timeout: 5) == .completed else {
            return nil
        }
        return input
    }

    private func configureLaunch(
        _ app: XCUIApplication,
        smokeAction: Int?,
        smokeText: String? = nil
    ) {
        app.launchArguments = ["-IDEALLUITesting"]
        if let smokeAction {
            app.launchArguments += ["-IDEALLSmokeAction", String(smokeAction)]
        }
        if let smokeText {
            app.launchArguments += ["-IDEALLSmokeText", smokeText]
        }
    }

    private func relaunch(
        _ app: XCUIApplication,
        smokeAction: Int,
        smokeText: String
    ) {
        app.terminate()
        configureLaunch(app, smokeAction: smokeAction, smokeText: smokeText)
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 20))
    }

    private func waitForAutosave() {
        Thread.sleep(forTimeInterval: 2)
    }

    private func assertFullScreen(_ appFrame: CGRect, matches deviceFrame: CGRect) {
        XCTAssertEqual(appFrame.minX, deviceFrame.minX, accuracy: 1)
        XCTAssertEqual(appFrame.minY, deviceFrame.minY, accuracy: 1)
        XCTAssertEqual(appFrame.width, deviceFrame.width, accuracy: 1)
        XCTAssertEqual(appFrame.height, deviceFrame.height, accuracy: 1)
    }
}
