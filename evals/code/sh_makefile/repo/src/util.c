#include "util.h"

int apply(long a, char op, long b, long *out)
{
	switch (op) {
	case '+': *out = a + b; return 0;
	case '-': *out = a - b; return 0;
	case '*': *out = a * b; return 0;
	case '/':
		if (b == 0)
			return -2;
		*out = a / b;
		return 0;
	}
	return -1;
}
