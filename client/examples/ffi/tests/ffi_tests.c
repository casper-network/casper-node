#include "CUnit/Basic.h"
#include "casper_client.h"

int init_suite() {
    casper_setup_client();
    return 0;
}

int clean_suite() {
    casper_shutdown_client();
    return 0;
}

void test_should_be_no_last_error_on_startup() {
    unsigned char error[255] = {0};
    int bytes_read = casper_get_last_error(error, 255);
    CU_ASSERT(0 == bytes_read);
}

void test_should_get_last_error_after_bad_request() {
    casper_deploy_params_t deploy_params = {0};
    deploy_params.secret_key = "resources/local/secret_keys/node-1.pem";
    deploy_params.ttl = "10s";
    deploy_params.chain_name = "casper-charlie-testnet1";
    deploy_params.gas_price = "11";

    casper_payment_params_t payment_params = {0};
    payment_params.payment_amount = "1000";

    const char *payment_args[2] = {
        "name_01:bool='false'",
        "name_02:i32='42'",
    };
    payment_params.payment_args_simple = (const char *const *)&payment_args;
    payment_params.payment_args_simple_len = 2;

    casper_session_params_t session_params = {0};
    session_params.session_path =
        "target/wasm32-unknown-unknown/release/standard_payment.wasm";

    unsigned char response_buffer[1024] = {0};

    casper_error_t success =
        casper_put_deploy("1", "", false, &deploy_params, &session_params,
                          &payment_params, response_buffer, 1024);

    CU_ASSERT_NOT_EQUAL(success, CASPER_SUCCESS);

    unsigned char error[255] = {0};
    int bytes_read = casper_get_last_error(error, 255);
    CU_ASSERT_STRING_EQUAL("failed to get rpc response: builder error: "
                           "relative URL without a base",
                           error);
}

int main(int argc, char **argv) {
    CU_pSuite pSuite = NULL;

    /* initialize the CUnit test registry */
    if (CUE_SUCCESS != CU_initialize_registry()) {
        return CU_get_error();
    }

    /* add a suite to the registry */
    pSuite =
        CU_add_suite("casper_client lib tests suite", init_suite, clean_suite);
    if (NULL == pSuite) {
        CU_cleanup_registry();
        return CU_get_error();
    }

    /* add the tests to the suite */
    if ((NULL == CU_add_test(pSuite, "should_be_no_last_error_on_startup",
                             test_should_be_no_last_error_on_startup)) ||
        (NULL == CU_add_test(pSuite, "should_get_last_error_after_bad_request",
                             test_should_get_last_error_after_bad_request))) {
        CU_cleanup_registry();
        return CU_get_error();
    }

    /* Run all tests using the CUnit Basic interface */
    CU_basic_set_mode(CU_BRM_VERBOSE);
    CU_basic_run_tests();
    CU_cleanup_registry();
    return CU_get_error();
}
