#!/usr/bin/env ruby
require "xcodeproj"
require "fileutils"

root = File.expand_path("..", __dir__)
project_path = File.join(root, "mac", "Damon.xcodeproj")
FileUtils.rm_rf(project_path)
project = Xcodeproj::Project.new(project_path)

app = project.new_target(:application, "Damon", :osx, "14.0")
unit = project.new_target(:unit_test_bundle, "DamonTests", :osx, "14.0")
ui = project.new_target(:ui_test_bundle, "DamonUITests", :osx, "14.0")
unit.add_dependency(app)
ui.add_dependency(app)

app_group = project.main_group.new_group("Damon", "Damon")
Dir.glob(File.join(root, "mac", "Damon", "**", "*.swift")).sort.each do |path|
  app.add_file_references([app_group.new_file(path.sub(File.join(root, "mac", "Damon") + "/", ""))])
end

tests_group = project.main_group.new_group("DamonTests", "DamonTests")
Dir.glob(File.join(root, "mac", "DamonTests", "*.swift")).sort.each do |path|
  unit.add_file_references([tests_group.new_file(path.sub(File.join(root, "mac", "DamonTests") + "/", ""))])
end

ui_group = project.main_group.new_group("DamonUITests", "DamonUITests")
Dir.glob(File.join(root, "mac", "DamonUITests", "*.swift")).sort.each do |path|
  ui.add_file_references([ui_group.new_file(path.sub(File.join(root, "mac", "DamonUITests") + "/", ""))])
end

app.build_configurations.each do |config|
  config.build_settings["PRODUCT_BUNDLE_IDENTIFIER"] = "com.savagemechanic.damon"
  config.build_settings["INFOPLIST_FILE"] = "Damon/Info.plist"
  config.build_settings["GENERATE_INFOPLIST_FILE"] = "NO"
  config.build_settings["SWIFT_VERSION"] = "6.0"
end
unit.build_configurations.each do |config|
  config.build_settings["PRODUCT_BUNDLE_IDENTIFIER"] = "com.savagemechanic.damon.tests"
  config.build_settings["GENERATE_INFOPLIST_FILE"] = "YES"
  config.build_settings["SWIFT_VERSION"] = "6.0"
  config.build_settings["TEST_HOST"] = "$(BUILT_PRODUCTS_DIR)/Damon.app/Contents/MacOS/Damon"
  config.build_settings["BUNDLE_LOADER"] = "$(TEST_HOST)"
end
ui.build_configurations.each do |config|
  config.build_settings["PRODUCT_BUNDLE_IDENTIFIER"] = "com.savagemechanic.damon.uitests"
  config.build_settings["GENERATE_INFOPLIST_FILE"] = "YES"
  config.build_settings["SWIFT_VERSION"] = "6.0"
  config.build_settings["TEST_TARGET_NAME"] = "Damon"
end

scheme = Xcodeproj::XCScheme.new
scheme.add_build_target(app)
scheme.add_test_target(unit)
scheme.add_test_target(ui)
scheme.set_launch_target(app)
scheme.save_as(project_path, "Damon", true)
project.save
