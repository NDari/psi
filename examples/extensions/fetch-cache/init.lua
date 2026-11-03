-- fetch-cache: a tool that fetches JSON from an HTTP API, cached on disk,
-- with settings in a TOML file. Shows psi.http (it fails cleanly when the
-- network setting is off), psi.fs, psi.hash, psi.base64, psi.toml and
-- psi.json.
--
-- Settings, in .psi/fetch-cache.toml (all optional):
--   max_age = 3600   # seconds a cached answer is used
--   [auth]
--   user = "me"      # basic auth, with the password from $FETCH_PASSWORD

local function settings(psi, cwd)
  local file = psi.path.join(cwd, ".psi/fetch-cache.toml")
  local s = psi.fs.exists(file) and psi.toml.decode(psi.fs.read(file)) or {}
  s.max_age = s.max_age or 3600
  return s
end

local function cache_dir(psi, cwd)
  return psi.path.join(cwd, ".cache/fetch")
end

local function fetch(psi, cwd, url)
  local s = settings(psi, cwd)
  local file = psi.path.join(cache_dir(psi, cwd), psi.hash.sha256(url) .. ".json")
  local st = psi.fs.stat(file)
  if st and psi.time.now() - st.modified < s.max_age then
    return psi.fs.read(file), true
  end
  local headers = { accept = "application/json" }
  if s.auth and s.auth.user then
    local password = os.getenv("FETCH_PASSWORD") or ""
    headers.authorization = "Basic " .. psi.base64.encode(s.auth.user .. ":" .. password)
  end
  local r = psi.http.request{ url = url, headers = headers, timeout_ms = 15000 }
  if not r.ok then error(url .. " answered " .. r.status) end
  local data = psi.json.decode(r.body)
  -- GitHub's contents API sends a file's text in base64, across lines.
  if type(data) == "table" and data.encoding == "base64" and type(data.content) == "string" then
    data.content = psi.base64.decode(psi.re.replace("\\s", data.content, ""))
    data.encoding = psi.json.null()
  end
  local body = psi.json.encode(data, { pretty = true })
  psi.fs.write(file, body)
  return body, false
end

return {
  name = "fetch-cache",
  version = "0.1.0",
  description = "Fetches JSON over HTTP, with a cache",
  setup = function(psi)
    psi.register_tool{
      name = "fetch_json",
      description = "Fetch JSON from an HTTP API; answers are cached for an hour",
      params = { url = { type = "string", description = "The http or https URL" } },
      annotations = { read_only = true, open_world = true },
      execute = function(args, ctx)
        local body, cached = fetch(psi, ctx.cwd, args.url)
        return (cached and "(cached)\n" or "") .. body
      end,
    }
    psi.register_command("fetch-cache", {
      description = "Show the cache and settings; `clear` empties the cache",
      handler = function(args, ctx)
        local dir = cache_dir(psi, ctx.cwd)
        if args == "clear" then
          for _, name in ipairs(psi.fs.exists(dir) and psi.fs.list(dir) or {}) do
            os.remove(psi.path.join(dir, name))
          end
          return ctx.ui.notify("cache cleared")
        end
        local files = psi.fs.exists(dir) and psi.fs.glob("*.json", { cwd = dir, gitignore = false }) or {}
        local bytes = 0
        for _, f in ipairs(files) do bytes = bytes + psi.fs.stat(psi.path.join(dir, f)).size end
        ctx.ui.notify(#files .. " cached answers, " .. bytes .. " bytes\n" .. psi.toml.encode(settings(psi, ctx.cwd)))
      end,
    })
  end,
}
