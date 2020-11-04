#include <stdio.h>

#include "casper_client.h"

#define RESPONSE_BUFFER_LEN 1024
#define ERROR_LEN 255

int main(int argc, char **argv) {
    const char *node_address = "http://localhost:50101";
    const char *rpc_id = "1";
    bool verbose = true;

    casper_setup_client();

    unsigned char response_buffer[RESPONSE_BUFFER_LEN] = {0};
    bool success =
        casper_get_auction_info(rpc_id, node_address, verbose, response_buffer,
                                RESPONSE_BUFFER_LEN);
    if (success == true) {
        printf("get_auction_info: got successful response\n%s\n",
               response_buffer);
    } else {
        unsigned char error[ERROR_LEN] = {0};
        casper_get_last_error(error, ERROR_LEN);
        printf("get_auction_info: error:\n%s\n", error);
    }
    printf("get_auction_info: done.\n");

    casper_shutdown_client();

    return 0;
}
