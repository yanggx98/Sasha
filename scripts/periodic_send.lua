-- Sasha 内置定时发送脚本
-- 默认每 1000 ms 发送一次数据，可参考 docs/lua-scripting-api.md 复制并自定义载荷与间隔。

local PAYLOAD = "AT\r\n"
local INTERVAL_MS = 1000

local count = 0
while not task.is_cancelled() do
    count = count + 1
    serial.send(PAYLOAD)
    log.info("第 " .. tostring(count) .. " 次定时发送")
    task.sleep(INTERVAL_MS)
end
