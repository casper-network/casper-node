#include <stdio.h>
#include "headers/casper_client.h"

int main(int argc, char **argv) {
    const char *node_address = "http://localhost:7777";
    const char *rpc_id = "1";
    bool verbose = true;
    unsigned char response_buffer[MAX_RESPONSE_BUFFER_LEN] = {0};
    bool success = get_auction_info(rpc_id, node_address, verbose, response_buffer, MAX_RESPONSE_BUFFER_LEN);
    if (success == true) {
        printf("C-client test: got successful response %s\n", response_buffer);
    } else {
        unsigned char error[MAX_ERROR_LEN] = {0};
        get_last_error(error, MAX_ERROR_LEN);
        printf("C-client test: got error response: %s\n", error);
    }
    printf("C-client test: done.\n");
    return 0;
}
