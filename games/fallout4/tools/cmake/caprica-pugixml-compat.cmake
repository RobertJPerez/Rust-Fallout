# Caprica's pinned CMakeLists references this target name, which current vcpkg
# pugixml packages no longer export. Keep the alias confined to the offline
# reference-tool build; it resolves to the package's supported imported target.
if(NOT TARGET pugixml::static)
  add_library(pugixml::static INTERFACE IMPORTED GLOBAL)
  set_property(TARGET pugixml::static PROPERTY INTERFACE_LINK_LIBRARIES pugixml::pugixml)
endif()
