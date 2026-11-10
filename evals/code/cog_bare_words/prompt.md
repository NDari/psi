Make bare words execute without the need for an explicit 'apply', including in hashes (like '#[ a: {dup *}] :x let 5 x.a' should be 25)
