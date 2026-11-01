<?php

// Spike 03: the raw commands behind the checks that fail on Garnet, run through predis.
//   php probe.php <app folder>
require $argv[1].'/vendor/autoload.php';

$r = new Predis\Client(['host' => '127.0.0.1', 'port' => 6379]);
$r->flushdb();
$show = function (string $what, $value) {
    echo str_pad($what, 58).' => '.json_encode($value)."\n";
};

// Cache::add: Laravel's RedisStore::add Lua script.
$add = "return redis.call('exists',KEYS[1])<1 and redis.call('setex',KEYS[1],ARGV[2],ARGV[1])";
$show('EVAL add-script on a missing key', $r->eval($add, 1, 'k1', 'v1', 60));
$show('  GET k1', $r->get('k1'));
$show('  TTL k1', $r->ttl('k1'));
$show('EVAL setex alone', $r->eval("return redis.call('setex',KEYS[1],ARGV[2],ARGV[1])", 1, 'k2', 'v2', 60));
$show('  GET k2', $r->get('k2'));
$show('EVAL set alone', $r->eval("return redis.call('set',KEYS[1],ARGV[1])", 1, 'k3', 'v3'));
$show('  GET k3', $r->get('k3'));
$show('EVAL set EX', $r->eval("return redis.call('set',KEYS[1],ARGV[1],'EX',ARGV[2])", 1, 'k4', 'v4', 60));
$show('  GET k4', $r->get('k4'));
$show('EVAL exists<1 and set', $r->eval("return redis.call('exists',KEYS[1])<1 and redis.call('set',KEYS[1],ARGV[1])", 1, 'k5', 'v5'));
$show('  GET k5', $r->get('k5'));
$show('SETEX directly', (string) $r->setex('k6', 60, 'v6'));
$show('  GET k6', $r->get('k6'));
$show('EVAL setex with a numeric TTL argument', $r->eval("return redis.call('setex',KEYS[1],tonumber(ARGV[2]),ARGV[1])", 1, 'k7', 'v7', 60));
$show('  GET k7', $r->get('k7'));

// Tag flush (Laravel's RedisTaggedCache): entries live in a sorted set per tag.
$show('ZADD tag entries', $r->zadd('tag:a:entries', [ 'key1' => 100, 'key2' => 200 ]));
$show('  ZRANGE', $r->zrange('tag:a:entries', 0, -1));
$show('  ZSCAN', $r->zscan('tag:a:entries', 0, ['count' => 100]));
$show('  ZRANGEBYSCORE 0 +inf', $r->zrangebyscore('tag:a:entries', 0, '+inf'));
$show('  ZADD GT (Laravel 11+ keeps the longest expiry)', $r->executeRaw(['ZADD', 'tag:a:entries', 'GT', '300', 'key1']));
$show('  ZSCORE key1 after GT', $r->zscore('tag:a:entries', 'key1'));
$show('  ZADD NX', $r->executeRaw(['ZADD', 'tag:a:entries', 'NX', '50', 'key3']));
