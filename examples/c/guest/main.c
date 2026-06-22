#include <hyperlight_guest.h>
#include <macro.h>
#include <stdio.h>

int say_hello(const char *string) {
  int n = printf("Hello %s\n", string);
  fflush(stdout);
  return n;
}

HYPERLIGHT_WRAP_FUNCTION(say_hello, Int, 1, String);
void hyperlight_main(void)
{
  HYPERLIGHT_REGISTER_FUNCTION("SayHello", say_hello);
}

hl_Vec *c_guest_dispatch_function(const hl_FunctionCall *function_call) {
  return NULL;
}
