# Run only in the CI checkout, after pod install. Do not change the app's shipped bundle ID.
require 'xcodeproj'

root = File.expand_path('..', __dir__)
project = Xcodeproj::Project.open(File.join(root, 'ShellfoxMobile.xcodeproj'))
app = project.targets.find { |target| target.name == 'ShellfoxMobile' } or abort 'Missing app target'
app.build_configurations.each do |config|
  config.build_settings['PRODUCT_BUNDLE_IDENTIFIER'] = 'com.shellfox.mobile.iosnativev1fixture'
end

tests = project.new_target(:ui_test_bundle, 'ShellfoxNativeV1UI', :ios, '15.1')
tests.add_dependency(app)
tests.add_file_references([project.main_group.new_file('tests/NativeV1UITests.swift')])
tests.build_configurations.each do |config|
  config.build_settings.update(
    'GENERATE_INFOPLIST_FILE' => 'YES',
    'PRODUCT_BUNDLE_IDENTIFIER' => 'com.shellfox.mobile.iosnativev1fixture.tests',
    'PRODUCT_NAME' => '$(TARGET_NAME)',
    'SWIFT_VERSION' => '5.0',
    'TARGETED_DEVICE_FAMILY' => '1,2',
    'TEST_TARGET_NAME' => app.name,
  )
end
project.save

scheme = Xcodeproj::XCScheme.new
scheme.add_build_target(app)
scheme.add_test_target(tests)
scheme.set_launch_target(app)
scheme.test_action.build_configuration = 'Release'
scheme.save_as(project.path, 'ShellfoxNativeV1UI', true)
