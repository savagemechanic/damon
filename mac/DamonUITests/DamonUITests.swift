import XCTest

final class DamonUITests: XCTestCase {
    @MainActor
    func testNativeWorkSurfacesAndSettings() throws {
        let app = XCUIApplication()
        app.launch()

        XCTAssertTrue(app.staticTexts["Conversation"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Activity"].exists)
        XCTAssertTrue(app.textFields["Ask Damon…"].exists)

        app.typeKey(",", modifierFlags: .command)
        XCTAssertTrue(app.buttons["Model"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.descendants(matching: .any)["zen-api-key"].exists)
        app.typeKey("w", modifierFlags: .command)

        app.menuBars.menuBarItems["Damon"].click()
        app.menuBars.menuBarItems["Damon"].menus.menuItems["Tools Library"].click()
        XCTAssertTrue(app.staticTexts["No reusable tools"].waitForExistence(timeout: 3))
    }
}
