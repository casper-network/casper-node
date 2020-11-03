#include <stdio.h>

#include "casper_client.h"

int main(int argc, char **argv) {
    const char *node_address = "http://localhost:7777";
    const char *rpc_id = "1";
    bool verbose = true;

    // TODO: fill in with some reasonable values
    casper_deploy_params_t deploy_params = {0};
    casper_payment_params_t payment_params = {0};
    casper_session_params_t session_params = {0};

    casper_setup_client();

    unsigned char response_buffer[CASPER_MAX_RESPONSE_BUFFER_LEN] = {0};
    bool success = casper_put_deploy(
        rpc_id, node_address, verbose, &deploy_params, &session_params,
        &payment_params, response_buffer, CASPER_MAX_RESPONSE_BUFFER_LEN);
    if (success == true) {
        printf("got successful response\n%s\n", response_buffer);
    } else {
        unsigned char error[CASPER_MAX_ERROR_LEN] = {0};
        casper_get_last_error(error, CASPER_MAX_ERROR_LEN);
        printf("error:\n%s\n", error);
    }
    printf("%s: done.\n", __FILE__);

    casper_shutdown_client();

    return 0;
}
