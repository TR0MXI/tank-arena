-- @name 小巡游者
-- @author Tank Arena
-- @description 沿弧线巡游；正前方出现敌车时，停下来射击。

-- 文件头的三项都可省略。每次决策只读 obs，返回想更新的操控项。
return function(obs)
	if obs.enemy_ahead then
		-- fire 模拟按键：可以开火时按下，下一次松开。
		return { drive = "stop", turn = "none", fire = obs.can_fire }
	end

	return { drive = "forward", turn = "left", fire = false }
end
