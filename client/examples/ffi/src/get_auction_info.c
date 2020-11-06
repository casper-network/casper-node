#include <stdio.h>

#include "casper_client.h"

#define RESPONSE_BUFFER_LEN 1024
#define ERROR_LEN 255
#define NODE_ADDRESS "http://localhost:50101"
#define RPC_ID "1"
#define VERBOSE false

int main(int argc, char **argv) {
    casper_setup_client();

    unsigned char response_buffer[RESPONSE_BUFFER_LEN] = {0};
    casper_error_t success = casper_get_auction_info(
        RPC_ID, NODE_ADDRESS, VERBOSE, response_buffer, RESPONSE_BUFFER_LEN);
    if (success == CASPER_SUCCESS) {
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
