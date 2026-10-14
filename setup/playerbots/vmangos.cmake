# Compile the complete upstream vanilla module; no strategies are excluded.
find_package(Boost 1.70 REQUIRED)
file(GLOB_RECURSE autonomous_bot_sources CONFIGURE_DEPENDS
  AutonomousBots/playerbot/*.cpp AutonomousBots/ahbot/*.cpp)
add_library(autonomous_playerbots OBJECT ${autonomous_bot_sources})
target_include_directories(autonomous_playerbots PRIVATE
  ${CMAKE_SOURCE_DIR}/src ${CMAKE_CURRENT_SOURCE_DIR}/AutonomousBots ${Boost_INCLUDE_DIRS})
target_link_libraries(autonomous_playerbots PRIVATE shared Detour g3dlite)
target_compile_definitions(autonomous_playerbots PRIVATE CMANGOS VMANGOS MANGOSBOT_ZERO ENABLE_PLAYERBOTS)
set_property(TARGET autonomous_playerbots PROPERTY CXX_STANDARD 20)
if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
  # Native core warning suppressions must not hide bot printf type/format errors.
  target_compile_options(autonomous_playerbots PRIVATE
    -Werror=format -Werror=format-security -Wno-format-zero-length)
endif()
option(PLAYERBOTS_USE_PCH "Precompile the bot module's shared compatibility headers" ON)
if(PLAYERBOTS_USE_PCH)
  target_precompile_headers(autonomous_playerbots PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/AutonomousBots/botpch.h)
elseif(MSVC)
  target_compile_options(autonomous_playerbots PRIVATE /FI${CMAKE_CURRENT_SOURCE_DIR}/AutonomousBots/botpch.h)
else()
  target_compile_options(autonomous_playerbots PRIVATE -include ${CMAKE_CURRENT_SOURCE_DIR}/AutonomousBots/botpch.h)
endif()
# Public definitions keep Player's layout consistent in all server translation units.
target_compile_definitions(game PUBLIC ENABLE_PLAYERBOTS)
target_compile_features(game PUBLIC cxx_std_20)
target_sources(game PRIVATE $<TARGET_OBJECTS:autonomous_playerbots>)
configure_file(AutonomousBots/playerbot/aiplayerbot.conf.dist.in
  ${CMAKE_CURRENT_BINARY_DIR}/aiplayerbot.conf.dist @ONLY)
configure_file(AutonomousBots/ahbot/ahbot.conf.dist.in
  ${CMAKE_CURRENT_BINARY_DIR}/playerbots-ahbot.conf.dist @ONLY)
install(FILES ${CMAKE_CURRENT_BINARY_DIR}/aiplayerbot.conf.dist
  ${CMAKE_CURRENT_BINARY_DIR}/playerbots-ahbot.conf.dist DESTINATION ${CONF_DIR})

install(FILES ${CMAKE_SOURCE_DIR}/sql/playerbots/characters.sql
  ${CMAKE_SOURCE_DIR}/sql/playerbots/world.sql DESTINATION share/vmangos/playerbots)

# Boost stacktrace uses dladdr on glibc versions before 2.34 as well.
# game uses CMake's plain target_link_libraries signature.
target_link_libraries(game ${CMAKE_DL_LIBS})
