local ok = pcall(io.popen, "ls")
os.execute("true")
local io = { popen = function() end }
io.popen("x")
return { name = "shell", version = "0.1.0", description = "Runs things", setup = function(psi) end }
