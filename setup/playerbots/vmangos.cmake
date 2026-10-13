# This experimental target deliberately includes the whole module. Do not omit failing
# strategies to make a partial port appear complete. Core lifecycle hooks are still pending.
find_package(Boost 1.70 REQUIRED)
file(GLOB_RECURSE autonomous_bot_sources CONFIGURE_DEPENDS
  AutonomousBots/playerbot/*.cpp AutonomousBots/ahbot/*.cpp)
add_library(autonomous_playerbots OBJECT ${autonomous_bot_sources})
target_include_directories(autonomous_playerbots PRIVATE
  ${CMAKE_SOURCE_DIR}/src ${CMAKE_CURRENT_SOURCE_DIR}/AutonomousBots ${Boost_INCLUDE_DIRS})
target_link_libraries(autonomous_playerbots PRIVATE shared Detour g3dlite)
target_compile_definitions(autonomous_playerbots PRIVATE CMANGOS VMANGOS MANGOSBOT_ZERO ENABLE_PLAYERBOTS)
set_property(TARGET autonomous_playerbots PROPERTY CXX_STANDARD 20)
if(MSVC)
  target_compile_options(autonomous_playerbots PRIVATE /FI${CMAKE_CURRENT_SOURCE_DIR}/AutonomousBots/botpch.h)
else()
  target_compile_options(autonomous_playerbots PRIVATE -include ${CMAKE_CURRENT_SOURCE_DIR}/AutonomousBots/botpch.h)
endif()
# No linkage into the runnable server until module compilation and lifecycle integration pass.
