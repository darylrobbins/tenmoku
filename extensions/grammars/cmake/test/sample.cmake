# Top-level build script for the demo project.
cmake_minimum_required(VERSION 3.20)
project(Demo VERSION 1.2.3 LANGUAGES C CXX)

#[[ A bracket comment
    spanning several lines. ]]

option(DEMO_BUILD_TESTS "Build the tests" ON)
set(CMAKE_CXX_STANDARD 17 CACHE STRING "C++ standard")
set(DEMO_SOURCES src/main.cpp src/util.cpp)
set(DEMO_BANNER "Demo \"v${PROJECT_VERSION}\"\n\tbuilt on $ENV{HOSTNAME}")
set(DEMO_RAW [=[raw text with ${no_expansion} inside]=])

list(APPEND DEMO_SOURCES src/extra.cpp)
list(LENGTH DEMO_SOURCES source_count)
message(STATUS "Found ${source_count} sources")

function(demo_add_tool name)
  add_executable(${name} ${ARGN})
  target_compile_definitions(${name} PRIVATE DEMO_TOOL=1)
  if(NOT TARGET ${name})
    message(FATAL_ERROR "failed to create ${name}")
  endif()
  return()
endfunction()

macro(demo_log text)
  message("demo: ${text}")
endmacro()

if(WIN32 AND NOT MINGW)
  add_compile_options(/W4)
elseif(CMAKE_CXX_COMPILER_ID STREQUAL "GNU" OR APPLE)
  add_compile_options(-Wall -Wextra)
else()
  demo_log("unknown compiler")
endif()

foreach(src IN LISTS DEMO_SOURCES)
  get_filename_component(stem ${src} NAME_WE)
  demo_log("source ${stem}")
  if(stem MATCHES "^extra")
    continue()
  endif()
endforeach()

set(attempt 0)
while(attempt LESS 3)
  math(EXPR attempt "${attempt} + 1")
  if(attempt EQUAL 2)
    break()
  endif()
endwhile()

add_library(demo STATIC ${DEMO_SOURCES})
target_include_directories(demo PUBLIC include)
demo_add_tool(demo_cli src/cli.cpp)
target_link_libraries(demo_cli PRIVATE demo)

if(DEMO_BUILD_TESTS)
  enable_testing()
  add_subdirectory(tests)
endif()

install(TARGETS demo demo_cli DESTINATION bin)
