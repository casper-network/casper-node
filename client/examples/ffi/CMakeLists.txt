cmake_minimum_required(VERSION 3.10)
project(CasperClientWrapper C)

find_program(CARGO cargo HINTS $ENV{HOME}/.cargo/bin)
if(NOT CARGO)
    message(FATAL_ERROR "cargo not found")
endif()

if(CMAKE_BUILD_TYPE STREQUAL Release)
    set(TARGET_DIR release)
else()
    set(TARGET_DIR debug)
endif()

# The casper_client library's name - e.g. 'libcasper_client.so' on Unix.
set(CLIENT_LIB_NAME ${CMAKE_SHARED_LIBRARY_PREFIX}casper_client${CMAKE_SHARED_LIBRARY_SUFFIX})
# The initial build location of the casper_client library after being built by cargo.
set(CLIENT_BUILT_LIB_SOURCE "${CMAKE_CURRENT_LIST_DIR}/../../../target/${TARGET_DIR}/${CLIENT_LIB_NAME}")
# The location of the casper_client library after being moved to the ffi examples build directory.
set(CLIENT_BUILT_LIB_TARGET "${CMAKE_CURRENT_BINARY_DIR}/lib/${CLIENT_LIB_NAME}")
# The initial location of the generated header for the casper_client library.
set(CLIENT_HEADERS_DIR_SOURCE "${CMAKE_CURRENT_LIST_DIR}/../../headers")
# The location of the casper_client header after being moved to the ffi examples build directory.
set(CLIENT_HEADERS_DIR_TARGET "${CMAKE_CURRENT_BINARY_DIR}/headers")

# This target builds the casper_client library and copies it and its header(s) to our build directory.
add_custom_target(
    ClientSharedLibrary
    COMMENT "Building casper_client library and copying from '${CLIENT_BUILT_LIB_SOURCE}' to '${CLIENT_BUILT_LIB_TARGET}'."
    COMMAND "${CARGO}" build --lib $<$<CONFIG:Release>:--release>
    COMMAND ${CMAKE_COMMAND} -E copy "${CLIENT_BUILT_LIB_SOURCE}" "${CLIENT_BUILT_LIB_TARGET}"
    COMMAND ${CMAKE_COMMAND} -E copy_directory "${CLIENT_HEADERS_DIR_SOURCE}" "${CLIENT_HEADERS_DIR_TARGET}"
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)

include_directories("${CLIENT_HEADERS_DIR_TARGET}")

add_executable(put-deploy src/put_deploy.c)
target_link_libraries(put-deploy PRIVATE ${CLIENT_BUILT_LIB_TARGET})
add_dependencies(put-deploy ClientSharedLibrary)

add_executable(get-auction-info src/get_auction_info.c)
target_link_libraries(get-auction-info PRIVATE ${CLIENT_BUILT_LIB_TARGET})
add_dependencies(get-auction-info ClientSharedLibrary)
