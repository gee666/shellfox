Pod::Spec.new do |s|
  s.name = 'ShellfoxRustNative'
  s.version = '0.1.0'
  s.summary = 'Shellfox native Ghostty terminal and remote Rust client bridge'
  s.homepage = 'https://github.com/oira666/shellfox'
  s.license = { :type => 'MIT' }
  s.author = 'Shellfox'
  s.source = { :git => 'https://github.com/oira666/shellfox.git', :tag => s.version.to_s }
  s.platform = :ios, min_ios_version_supported
  s.source_files = '*.{h,mm,inc}'
  s.requires_arc = true
  s.public_header_files = 'ShellfoxRustTerminalComponentView.h'
  s.frameworks = 'UIKit', 'CoreText'
  s.libraries = 'c++'
  s.dependency 'ShellfoxRustRuntime'
  s.dependency 'ShellfoxEmbeddedNetwork'
  s.pod_target_xcconfig = {
    'CLANG_CXX_LANGUAGE_STANDARD' => 'c++20',
    'GCC_ENABLE_CPP_EXCEPTIONS' => 'YES',
    'HEADER_SEARCH_PATHS' => '$(inherited) "$(PODS_ROOT)/Headers/Private/ReactCodegen"'
  }
  install_modules_dependencies(s)
  s.test_spec 'NativeContracts' do |tests|
    tests.source_files = 'Tests/*.{h,mm}'
    tests.resources = 'Tests/Fixtures/*.json'
    tests.frameworks = 'XCTest'
    tests.requires_app_host = true
  end
end
