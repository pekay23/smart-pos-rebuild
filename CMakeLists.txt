cmake_minimum_required(VERSION 3.20)
project(SmartPOS_Server VERSION 1.5.0 LANGUAGES C)
enable_language(RC)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()

if(NOT WIN32)
    message(FATAL_ERROR "SmartPOS Server is a native Windows application")
endif()

if(NOT CMAKE_SIZEOF_VOID_P EQUAL 4)
    message(FATAL_ERROR "Configure with a 32-bit toolchain (for Visual Studio use -A Win32)")
endif()

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_C_EXTENSIONS OFF)

add_executable(SmartPOS-Server WIN32
    src/main.c
    src/window_main.c
    resources/recovered.rc
)
target_include_directories(SmartPOS-Server PRIVATE include src)
target_compile_definitions(SmartPOS-Server PRIVATE
    WINVER=0x0601 _WIN32_WINNT=0x0601 UNICODE _UNICODE NOMINMAX STRICT
)
target_link_libraries(SmartPOS-Server PRIVATE
    comctl32 ole32 shell32 advapi32 user32 gdi32 comdlg32 shlwapi
)

if(MSVC)
    target_compile_options(SmartPOS-Server PRIVATE /W4 /utf-8)
    target_link_options(SmartPOS-Server PRIVATE /MANIFEST:NO)
else()
    target_compile_options(SmartPOS-Server PRIVATE -Wall -Wextra -Wno-unused-parameter)
endif()

set_target_properties(SmartPOS-Server PROPERTIES
    OUTPUT_NAME "smart-pos-server-rebuild"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/app"
)
