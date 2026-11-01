<?php

// Spike 03: do Lua scripts on Garnet act on the selected database? Laravel's cache store
// uses database 1 (REDIS_CACHE_DB), its queue database 0.
//   php probe-db.php <app folder>
require $argv[1].'/vendor/autoload.php';

$show = function (string $what, $value) {
    echo str_pad($what, 58).' => '.json_encode($value)."\n";
};
$db0 = new Predis\Client(['host' => '127.0.0.1', 'port' => 6379, 'database' => 0]);
$db1 = new Predis\Client(['host' => '127.0.0.1', 'port' => 6379, 'database' => 1]);
$db0->flushall();

$show('db1: SET plain', (string) $db1->set('plain', 'p'));
$show('  db1 GET plain / db0 GET plain', [$db1->get('plain'), $db0->get('plain')]);

$show('db1: EVAL set', $db1->eval("return redis.call('set',KEYS[1],ARGV[1])", 1, 'scripted', 's'));
$show('  db1 GET scripted / db0 GET scripted', [$db1->get('scripted'), $db0->get('scripted')]);

$show('db1: EVAL get of a key SET in db1', $db1->eval("return redis.call('get',KEYS[1])", 1, 'plain'));
$show('db1: EVAL del of a key SET in db1', $db1->eval("return redis.call('del',KEYS[1])", 1, 'plain'));
$show('  db1 GET plain after', $db1->get('plain'));

$show('db1: ZADD, then EVAL zrem', [$db1->zadd('z', [ 'm' => 1 ]), $db1->eval("return redis.call('zrem',KEYS[1],ARGV[1])", 1, 'z', 'm')]);
$show('  db1 ZRANGE z after', $db1->zrange('z', 0, -1));
