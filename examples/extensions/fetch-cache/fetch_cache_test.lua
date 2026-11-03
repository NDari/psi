local URL = "https://api.example.com/repos/me/app/contents/a.txt"

test.case("fetches, decodes base64 content and caches the answer", function()
  test.ui.answer("http", { status = 200, body = '{"name":"a.txt","encoding":"base64","content":"aGVs\\nbG8="}' })
  local r = test.tool("fetch_json", { url = URL })
  test.eq(r.is_error, false)
  test.contains(r.text, '"content": "hello"')
  test.contains(r.text, '"encoding": null')
  test.eq(test.requests("http")[1].url, URL)
  -- The second time comes from the cache, without a request.
  local again = test.tool("fetch_json", { url = URL })
  test.contains(again.text, "(cached)")
  test.eq(#test.requests("http"), 1)
end)

test.case("a failed request is a failed tool call", function()
  test.ui.answer("http", { status = 404, body = "" })
  local r = test.tool("fetch_json", { url = "https://api.example.com/missing" })
  test.truthy(r.is_error)
  test.contains(r.text, "answered 404")
end)

test.case("the command shows the cache and the settings", function()
  psi.fs.write(psi.path.join(test.tmp, ".psi/fetch-cache.toml"), 'max_age = 60\n[auth]\nuser = "me"\n')
  test.command("/fetch-cache", "")
  local shown = test.requests("notify")[1].message
  test.contains(shown, "1 cached answers")
  test.contains(shown, "max_age = 60")
  test.contains(shown, 'user = "me"')
  test.command("/fetch-cache", "clear")
  test.command("/fetch-cache", "")
  test.contains(test.requests("notify")[3].message, "0 cached answers")
end)
