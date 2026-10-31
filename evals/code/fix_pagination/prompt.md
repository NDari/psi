Something is wrong with our pagination (pager.py, used by the /users endpoint in api.py). There are 45 users; with per_page=10, page 1 starts at user 11 instead of user 1, and our client (fetch_all) stops after page 4, so some users never show up at all.

Pages are numbered from 1, total_pages should be the number of pages needed to hold every item (0 when there are none), and a page past the last one is just empty. Please fix it. The tests are in tests/, run python3 -m unittest.
