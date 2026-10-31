/* Lua 5.4's math.cosh and math.sinh (LUA_COMPAT_MATHLIB, always on in the
   vendored build) would otherwise bind to the build machine's newest glibc
   versions of these functions (2.44 on Arch), which older systems such as
   the Debian eval image (2.41) don't have. Defining them here keeps psi
   runnable there. Only those two deprecated Lua functions use them. */
extern double exp(double);
extern double expm1(double);

double cosh(double x) {
    double e = exp(x);
    return (e + 1.0 / e) / 2.0;
}

double sinh(double x) {
    /* expm1 keeps small arguments accurate. */
    return (expm1(x) - expm1(-x)) / 2.0;
}
