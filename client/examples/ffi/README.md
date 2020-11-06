# casper_client ffi examples

It's possible to use the `casper-client` library from C: 

## Building with cmake

To build the examples, run:
```
> mkdir -p build && pushd build && cmake .. && make && popd
```

In the `build` directory you created, you should see the binaries for the examples that have been compiled.

The build also produce a shared library in `<project_root>/libcasper_client.so`. The header can then be included from `<project_root>/client/headers/casper_client.h`.
```
#include "casper_client.h
```

## Initial setup
Some resources need to be initialized before library functions can be called:
```
/* initialize casper-client library */
casper_setup_client();
```

After this, it's possible to call library functions to query the node.

For example:

```
  unsigned char response_buffer[RESPONSE_BUFFER_LEN] = {0};
  casper_error_t response_code = casper_get_auction_info(RPC_ID, NODE_ADDRESS, VERBOSE,
                                         response_buffer, RESPONSE_BUFFER_LEN);
  if (response_code == CASPER_SUCCESS) {
    printf("get_auction_info: got successful response\n%s\n", response_buffer);
  } else {
      /* handle error... see Error Handling below */
  }
```

## Error Handling

Errors are returned from the various library functions as `casper_error_t`, but more detail can be gathered using `get_last_error`, which will pull the last error that occurred in the library as a string.
```
if (response == CASPER_IO_ERROR) {
    /* first, initialize a buffer to hold our error string */
    unsigned char error[ERROR_LEN] = {0};

    /* ask for the description of the latest error, which was a CASPER_IO_ERROR in this case */
    casper_get_last_error(error, ERROR_LEN);

    printf("got an IO error:\n%s\n", error);
}
```

Refer to `<project_root>/client/headers/casper_client.h` as well as the examples in the `src` directory
for more information about specific functions and their arguments.

## Error handling
Error handling : TODO with enumeration of error types!!!!

## Cleanup
In order to clean up and free any resources that the library has allocated, run:
```
/* finally, clean up after ourselves */
casper_shutdown_client();
```