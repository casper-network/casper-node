#include "casper_client.h"
#include <stdio.h>

int main(int argc, char **argv) {
  const char *node_address = "http://localhost:7777";
  const char *rpc_id = "1";
  bool verbose = true;
  unsigned char response_buffer[MAX_RESPONSE_BUFFER_LEN] = {0};
  bool success = get_auction_info(rpc_id, node_address, verbose,
                                  response_buffer, MAX_RESPONSE_BUFFER_LEN);
  if (success == true) {
    printf("get_auction_info: got successful response\n%s\n", response_buffer);
  } else {
    unsigned char error[MAX_ERROR_LEN] = {0};
    get_last_error(error, MAX_ERROR_LEN);
    printf("get_auction_info: got error response:\n%s\n", error);
  }
  printf("get_auction_info: done.\n");
  return 0;
}
