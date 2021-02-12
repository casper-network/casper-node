cmake_minimum_required(VERSION 3.10)
project(CasperClientWrapper C)

find_program(Cargo cargo HINTS $ENV{HOME}/.cargo/bin)
if(NOT Cargo)
    message(FATAL_ERROR "cargo not found")
endif()

if(CMAKE_BUILD_TYPE STREQUAL Release)
    set(TARGET_DIR release)
else()
    set(TARGET_DIR debug)
endif()

# The root path to install casper_client and Unity test lib.
set(InstallPath "${CMAKE_CURRENT_BINARY_DIR}/installed")
# The casper_client library's name - e.g. 'libcasper_client.so' on Unix.
set(ClientLibName ${CMAKE_SHARED_LIBRARY_PREFIX}casper_client${CMAKE_SHARED_LIBRARY_SUFFIX})
# The initial build location of the casper_client library after being built by cargo.
set(ClientBuiltLibSource "${CMAKE_CURRENT_LIST_DIR}/../../../target/${TARGET_DIR}/${ClientLibName}")
# The location of the casper_client library after being moved to the ffi examples build directory.
set(ClientBuiltLibTarget "${InstallPath}/lib/${ClientLibName}")
# The initial location of the generated header for the casper_client library.
set(ClientHeadersDirSource "${CMAKE_CURRENT_LIST_DIR}/../../../target/headers")
# The location of the casper_client header after being moved to the ffi examples build directory.
set(ClientHeadersDirTarget "${InstallPath}/include")

# This target builds the casper_client library and copies it and its header(s) to our build directory.
add_custom_target(
    ClientSharedLibrary
    COMMENT "Building casper_client library and copying from '${ClientBuiltLibSource}' to '${ClientBuiltLibTarget}'."
    COMMAND "${Cargo}" build --lib $<$<CONFIG:Release>:--release>
    COMMAND ${CMAKE_COMMAND} -E copy "${ClientBuiltLibSource}" "${ClientBuiltLibTarget}"
    COMMAND ${CMAKE_COMMAND} -E copy_directory "${ClientHeadersDirSource}" "${ClientHeadersDirTarget}"
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)

include_directories("${ClientHeadersDirTarget}")

add_executable(put-deploy src/put_deploy.c)
target_link_libraries(put-deploy PRIVATE ${ClientBuiltLibTarget})
add_dependencies(put-deploy ClientSharedLibrary)

add_executable(get-auction-info src/get_auction_info.c)
target_link_libraries(get-auction-info PRIVATE ${ClientBuiltLibTarget})
add_dependencies(get-auction-info ClientSharedLibrary)

include(ExternalProject)
ExternalProject_Add(Unity
    URL https://github.com/ThrowTheSwitch/Unity/archive/v2.5.1.tar.gz
    CMAKE_ARGS -DCMAKE_INSTALL_PREFIX:PATH=${InstallPath}
)
set(UnityLibName ${CMAKE_STATIC_LIBRARY_PREFIX}unity${CMAKE_STATIC_LIBRARY_SUFFIX})

add_executable(ffi-tests tests/ffi_tests.c)
target_link_libraries(ffi-tests PRIVATE ${ClientBuiltLibTarget} ${UnityLibName})
target_include_directories(ffi-tests PRIVATE "${ClientHeadersDirTarget}" "${InstallPath}/include/unity")
add_dependencies(ffi-tests ClientSharedLibrary Unity)
