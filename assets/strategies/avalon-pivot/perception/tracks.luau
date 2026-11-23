--!strict
local Frame = require("../model/frame")
local Geo = require("../model/geometry")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

type Obs = Types.Obs
type Tank = Types.Tank
type Track = Types.Track

local track: {Track} = {}
-- 坦克表由宿主复用；保存连续公开坐标，不读取敌人的按键或未来状态。
@native
local function updateTracks(o: Obs, now: number)
    local wing = false
    for _, t in o.tanks do
        if t.alive and t.team_id == o.me.team_id and t.slot_id ~= o.me.slot_id then wing = true; break end
    end
    for _, t in o.tanks do
        if t.alive then
            local m = track[t.slot_id]
            if not m then
                local initial: Track = {
                    x = t.x,
                    y = t.y,
                    at = now,
                    vx = 0,
                    vy = 0,
                    covered = false,
                    coastBudget = Tuning.HORIZON_TICKS,
                    heading = t.heading,
                    loaded = Frame.capacity,
                    refill = math.huge,
                }
                m = initial
                track[t.slot_id] = initial
            elseif now > m.at then
                m.previousLoad = m.loaded
                local dt = now - m.at
                local vx, vy = (t.x - m.x) / dt, (t.y - m.y) / dt
                local unstable = t.heading ~= m.heading or math.abs(vx-m.vx) + math.abs(vy-m.vy) > 0.08 * o.forward_speed
                m.coastBudget = if unstable then 12 else math.min(Tuning.HORIZON_TICKS, m.coastBudget + 4*dt)
                m.heading, m.vx, m.vy = t.heading, vx, vy
                m.x, m.y, m.at = t.x, t.y, now
            end
            if not wing then m.coastBudget = Tuning.HORIZON_TICKS end
            m.loaded = Frame.capacity
            m.refill = math.huge
            m.coast = nil
            m.covered = false
        else
            track[t.slot_id] = nil
        end
    end
    for _, b in o.shells do
        local m = track[b.owner]
        if m then
            m.loaded -= 1
            if m.previousLoad and m.loaded < m.previousLoad then
                m.firedAt = now
            end
            m.refill = math.min(m.refill, b.life)
        end
    end
end

local function armed(t: Tank)
    local m = track[t.slot_id]
    return m.loaded > 0
end

-- 沿最近测到的速度预判，但不要把车身外推穿过墙角。
@native
local function coastTime(t: Tank, m: Track): number
    if not m.coast then
        local horizon = m.coastBudget -- 按公开速度外推，到墙即停。
        m.coast = if m.vx == 0 and m.vy == 0
            then 0
            else horizon * (Geo.cast(t.x, t.y, m.vx * horizon, m.vy * horizon, Frame.hull) or 1)
    end
    return m.coast :: number
end

@native
local function predict(t: Tank, dt: number): (number, number)
    local m = track[t.slot_id]
    dt = math.min(dt, coastTime(t, m))
    return t.x + m.vx * dt, t.y + m.vy * dt
end

return {
    armed = armed,
    coastTime = coastTime,
    bySlot = track,
    predict = predict,
    update = updateTracks,
}
