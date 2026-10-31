. /evallib/check.sh
unittests /hidden/test_hidden_billing.py
check_output render_sample_order /hidden/want_0412.txt python3 -m billing /hidden/order_0412.json
check_output render_other_order /hidden/want_b.txt python3 -m billing /hidden/order_b.json
