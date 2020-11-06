cmake_minimum_required(VERSION 3.10)

find_program(CARGO "cargo" HINTS $ENV{HOME}/.cargo/bin)
if(NOT CARGO)
  message(FATAL_ERROR "Cargo not found")
endif()

project(casper_client_wrapper, C)

if (CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(CARGO_CMD cargo build)
    set(TARGET_DIR "debug")
else ()
    set(CARGO_CMD cargo build --release)
    set(TARGET_DIR "release")
endif ()

set(CLIENT_SO "${CMAKE_CURRENT_BINARY_DIR}/../../../../target/${TARGET_DIR}/libcasper_client.so")
set(HEADER_DIR "${CMAKE_CURRENT_BINARY_DIR}/../../../headers")

add_custom_target(
    RunCargoBuild
    COMMENT "Running cargo to compile client library and generate headers."
    COMMAND ${CARGO_CMD}
    COMMAND cp ${CLIENT_SO} ${CMAKE_CURRENT_BINARY_DIR}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)

include_directories(${HEADER_DIR})

add_executable(put_deploy src/put_deploy.c)
add_dependencies(put_deploy RunCargoBuild)
target_link_libraries(put_deploy ${CLIENT_SO})

add_executable(get_auction_info src/get_auction_info.c)
add_dependencies(get_auction_info RunCargoBuild)
target_link_libraries(get_auction_info ${CLIENT_SO})