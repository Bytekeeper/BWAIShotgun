[![Build status](https://ci.appveyor.com/api/projects/status/3j406t3yd3oymiqy?svg=true)](https://ci.appveyor.com/project/Bytekeeper/bwaishotgun)

# BWAIShotgun
Utility to quickly setup Starcraft Broodwar matches between 2 *or more* bots

Be aware that all bots will be executed directly, without any layer of isolation. If you need a more secure environment, either use sc-docker or setup BWAIShotgun in a VM.

# Usage
## Setup `BWAIshotgun`
Download the latest release of `bwaishotgun.z7` and unpack it.
Since Virus Scanners might interfere with download, it is password protected with the password `shotgun`.
The `bwheadless` file inside might trigger your Virus Scanner directly or indirectly (when bwaishotgun is started).

I built the binary using a [fork](https://github.com/Bytekeeper/bwheadless) of the origin [bwheadless](https://github.com/tscmoo/bwheadless),
feel free to check the code. It certainly does fishy things, which is to be expected as it heavily modifies StarCraft to run without UI etc.
I also modified it to run with "normal" game speed (LF3) - because most bots expect that.

## Setup the Game
Have an installation of StarCraft Broodwar 1.16 (or get it [here](http://www.cs.mun.ca/~dchurchill/startcraft/scbw_bwapi440.zip)).

Copy the `SNP_DirectIP.snp` (Local PC network inside the game) - the modified version of BWAIshotgun allows for 8 bots to play in a single game.

## Configure BWAIshotgun
Edit the `shotgun.toml` file. Many newer Java bots should run with any odd Java you have installed.
In that case, just leave the java setting open, and try the version on the `PATH`.

Download bots of your choice (only BWAPI 4.2+ bots were tested) from https://www.sscaitournament.com/index.php?action=scores.
Inside the bots directory, copy the `template` directory and rename it to the bot. 
Place the `BWAPI.dll` inside, and the bot binary inside the `bwapi-data\AI` folder.

To setup a game, edit the `game.toml` file. Add the absolute path of the map you want, and setup the bots.
The description of the `game_type` variable should be sufficient.

## Setup a sandbox
Ladders like SSCAIT and BASIL are using virtualization solutions. 
You might want to protect your computer from malicious code in bots as well.
Consider setting up a sandbox (like [Sandboxie](https://sandboxie-plus.com/)) or a virtual machine.

## Running BWAIshotgun

Finally, run `bwaishotgun.exe` - it should show some info output of bots being started.
There is currently no timeout mechanism. 
If the game does not stop after a few minutes, kill it and check the `logs` folder inside each bot folder for errors.

After the game ran, check the `replays` folder for each bot - they should contain the replay from that bots perspective.

If a bot fails to work, feel free to open an issue - please include a zipped up version of that bots directory. 
Bots older that BWAPI 4.2 might need some more setup, please make sure that it can run without `bwaishotgun`, before opening a ticket.

# Additional Artifact Sources
[bwheadless](https://github.com/Bytekeeper/bwheadless)
[Tournament Modules](https://github.com/basil-ladder/sc-tm)
[WMode](https://github.com/bwapi/bwapi/blob/main/Release_Binary/Chaoslauncher/Plugins/WModeReadme.txt)
[injectory](https://github.com/blole/injectory)