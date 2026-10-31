#ifndef UTIL_H
#define UTIL_H

/* apply computes a OP b into *out. It returns 0 on success, -1 for an
 * unknown operator and -2 for division by zero. */
int apply(long a, char op, long b, long *out);

#endif
