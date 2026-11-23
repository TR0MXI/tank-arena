--!strict
-- 策略参数只取决于比赛 tick，不依赖墙钟。
local HORIZON_TICKS, SLICE = 60, 3
return table.freeze({
    HORIZON_TICKS = HORIZON_TICKS, SLICE = SLICE, SAMPLES = HORIZON_TICKS / SLICE, BUFFER = 77, COMMIT = 7, FEINT = 1.2,
    -- 只用于路口选择的有界偏好，不扰动瞬时操纵与开火。
    ROUTE_TEMP = 0.92,
})
