class CPlayer
{
	var currentWeapon : CWeapon;
	var health : int;

	function Attack() : int
	{
		return currentWeapon.$0GetDamage();
	}

	function TakeDamage(amount : int)
	{
		health -= amount;
	}

	function ResolveHit(out result : int, optional crit : bool, optional out log : string)
	{
		result = health;
		if (crit)
		{
			result += 1;
		}
		log = "ok";
	}

	function Tick()
	{
		var result : int;
		var log : string;
		TakeDamage(50);
		ResolveHit(result, true, log);
	}
}
