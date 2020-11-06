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

# Note that this build file can be invoked from <project_root>/client/build.rs
# in order to hook into CI. In that case we set ONLY_COPY_CLIENT_SO
if (DEFINED CMAKE_CASPER_CLIENT_LIB_PREBUILT)
message(WARNING "Will copy *prebuilt* client library to build directory")
add_custom_target(
    ClientSharedLibrary 
    COMMENT "Copying *prebuilt* client library to build directory"
    COMMAND cp ${CLIENT_SO} ${CMAKE_CURRENT_BINARY_DIR}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)
else ()
# By default, rebuild the client library from scratch.
add_custom_target(
    ClientSharedLibrary 
    COMMENT "Running '${CARGO_CMD}' to compile client library and generate headers."
    COMMAND ${CARGO_CMD}
    COMMAND cp ${CLIENT_SO} ${CMAKE_CURRENT_BINARY_DIR}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)
endif ()

include_directories(${HEADER_DIR})

add_executable(put_deploy src/put_deploy.c)
add_dependencies(put_deploy ClientSharedLibrary)
target_link_libraries(put_deploy ${CLIENT_SO})

add_executable(get_auction_info src/get_auction_info.c)
add_dependencies(get_auction_info ClientSharedLibrary)
target_link_libraries(get_auction_info ${CLIENT_SO})