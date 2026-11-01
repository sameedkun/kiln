<?php

// Spike 03 (docs/spikes/03-windows-cache.md): everything Laravel uses Redis for, against the
// server REDIS_HOST/REDIS_PORT point at. Copied into a fresh Laravel app as
// app/Console/Commands/KilnSpike.php; run with `php artisan kiln:spike`. Prints one
// PASS/FAIL line per check and exits 1 if any failed.

namespace App\Console\Commands;

use Illuminate\Bus\Queueable;
use Illuminate\Console\Command;
use Illuminate\Contracts\Queue\ShouldQueue;
use Illuminate\Foundation\Bus\Dispatchable;
use Illuminate\Queue\InteractsWithQueue;
use Illuminate\Queue\SerializesModels;
use Illuminate\Support\Facades\Artisan;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Queue;
use Illuminate\Support\Facades\RateLimiter;
use Illuminate\Support\Facades\Redis;

class SpikeJob implements ShouldQueue
{
    use Dispatchable, InteractsWithQueue, Queueable, SerializesModels;

    public $tries = 2;

    public function __construct(public string $marker, public string $mode = 'ok') {}

    public function handle(): void
    {
        if ($this->mode === 'fail') {
            throw new \RuntimeException('failing on purpose');
        }
        if ($this->mode === 'release' && $this->attempts() === 1) {
            $this->release(1);

            return;
        }
        Cache::store('redis')->put($this->marker, 'done:'.$this->attempts(), 60);
    }
}

class KilnSpike extends Command
{
    protected $signature = 'kiln:spike';

    protected $description = 'Spike 03: Laravel against a Redis-compatible server';

    private int $failed = 0;

    private function check(string $name, callable $test): void
    {
        $started = microtime(true);
        try {
            $result = $test();
            $ms = (int) ((microtime(true) - $started) * 1000);
            if ($result === true) {
                $this->line("PASS {$name} ({$ms} ms)");
            } else {
                $this->failed++;
                $this->line("FAIL {$name}: got ".var_export($result, true));
            }
        } catch (\Throwable $e) {
            $this->failed++;
            $this->line("FAIL {$name}: ".get_class($e).': '.$e->getMessage());
        }
    }

    private function work(): void
    {
        Artisan::call('queue:work', ['connection' => 'redis', '--stop-when-empty' => true, '--sleep' => 0, '--tries' => 2]);
    }

    public function handle(): int
    {
        $cache = Cache::store('redis');
        $info = Redis::connection()->command('info', ['server']);
        $this->line('server: '.json_encode(is_array($info) ? ($info['Server'] ?? $info) : $info));
        Redis::connection()->command('flushdb');

        // Cache basics.
        $this->check('cache put/get/has/forget', function () use ($cache) {
            $cache->put('a', ['x' => 1], 60);

            return $cache->get('a') === ['x' => 1] && $cache->has('a') && $cache->forget('a') && ! $cache->has('a');
        });
        // RedisStore::add is a Lua script (EXISTS, then SETEX). Numbers come back as strings
        // from the Redis store, so a string value is compared.
        $this->check('cache add only when missing (Lua)', function () use ($cache) {
            $seen = [$cache->add('b', 'first', 60), $cache->add('b', 'second', 60), $cache->get('b')];

            return $seen === [true, false, 'first'] ? true : $seen;
        });
        $this->check('cache increment/decrement', function () use ($cache) {
            $cache->put('n', 5, 60);

            return $cache->increment('n', 3) === 8 && $cache->decrement('n') === 7;
        });
        $this->check('cache many / putMany', function () use ($cache) {
            $cache->putMany(['m1' => 'one', 'm2' => 'two'], 60);

            return $cache->many(['m1', 'm2', 'm3']) === ['m1' => 'one', 'm2' => 'two', 'm3' => null];
        });
        $this->check('cache remember and forever', fn () => $cache->remember('r', 60, fn () => 'computed') === 'computed'
            && $cache->remember('r', 60, fn () => 'again') === 'computed'
            && $cache->forever('f', 'kept') && $cache->get('f') === 'kept');
        $this->check('cache expiry (TTL 1 s)', function () use ($cache) {
            $cache->put('ttl', 'soon', 1);
            usleep(2100000);

            return $cache->get('ttl') === null;
        });
        $this->check('cache flush (FLUSHDB)', function () use ($cache) {
            $cache->put('gone', 1, 60);
            $cache->flush();

            return $cache->get('gone') === null;
        });

        // Locks: owner-checked release is a Lua script.
        $this->check('lock: second holder refused, owner release (Lua)', function () use ($cache) {
            $lock = $cache->lock('job-lock', 10);
            $other = $cache->lock('job-lock', 10);
            $got = $lock->get();
            $refused = ! $other->get();
            $notOwner = ! $other->release();
            $released = $lock->release();

            return $got && $refused && $notOwner && $released && $other->get() && $other->release();
        });
        $this->check('lock: block() waits for a lock that expires', function () use ($cache) {
            $cache->lock('short', 1)->get();
            $waited = $cache->lock('short', 5)->block(3, fn () => true);

            return $waited === true;
        });
        $this->check('lock: restoreLock and forceRelease', function () use ($cache) {
            $lock = $cache->lock('owned', 10);
            $lock->get();
            $restored = $cache->restoreLock('owned', $lock->owner());
            $ok = $restored->release();
            $cache->lock('forced', 10)->get();
            $cache->lock('forced')->forceRelease();

            return $ok && $cache->lock('forced', 10)->get();
        });

        // Tags.
        $this->check('tags: put, get, flush one tag', function () use ($cache) {
            $cache->tags(['people', 'artists'])->put('john', 'J', 60);
            $cache->tags(['people', 'authors'])->put('anne', 'A', 60);
            $seen = ['john before' => $cache->tags(['people', 'artists'])->get('john')];
            $cache->tags(['artists'])->flush();
            $seen['john after'] = $cache->tags(['people', 'artists'])->get('john');
            $seen['anne after'] = $cache->tags(['people', 'authors'])->get('anne');

            return $seen === ['john before' => 'J', 'john after' => null, 'anne after' => 'A'] ? true : $seen;
        });
        $this->check('tags: flushStale (cache:prune-stale-tags)', function () {
            return Artisan::call('cache:prune-stale-tags', ['store' => 'redis']) === 0;
        });

        // Sessions.
        // The session handler Laravel uses for SESSION_DRIVER=redis, written and read
        // directly, as a request's start and end do.
        $this->check('sessions: written and read back by id', function () {
            $handler = app('session')->driver('redis')->getHandler();
            $id = str_repeat('a', 40);
            $payload = serialize(['cart' => ['apple', 'pear']]);
            $wrote = $handler->write($id, $payload);
            $read = $handler->read($id);
            $handler->destroy($id);
            $gone = $handler->read($id);

            return [$wrote, $read, $gone] === [true, $payload, ''] ? true : [$wrote, $read, $gone];
        });

        // Queue: push, pop (Lua), delayed (sorted set + Lua migrate), release, fail.
        $this->check('queue: a job runs (push, Lua pop)', function () use ($cache) {
            Queue::connection('redis')->push(new SpikeJob('job-now'));
            $size = Queue::connection('redis')->size();
            $this->work();

            return $size === 1 && $cache->get('job-now') === 'done:1';
        });
        $this->check('queue: a delayed job waits, then runs (Lua migrate)', function () use ($cache) {
            Queue::connection('redis')->later(2, new SpikeJob('job-later'));
            $this->work();
            $early = $cache->get('job-later');
            usleep(2500000);
            $this->work();

            return $early === null && $cache->get('job-later') === 'done:1';
        });
        $this->check('queue: a released job runs again', function () use ($cache) {
            Queue::connection('redis')->push(new SpikeJob('job-release', 'release'));
            $this->work();
            usleep(1500000);
            $this->work();

            return $cache->get('job-release') === 'done:2';
        });
        $this->check('queue: a failing job ends in failed_jobs', function () {
            $before = DB::table('failed_jobs')->count();
            Queue::connection('redis')->push(new SpikeJob('job-fail', 'fail'));
            $this->work();

            return DB::table('failed_jobs')->count() === $before + 1;
        });

        // Rate limiting.
        $this->check('rate limiter: 3 allowed, the 4th refused', function () {
            $results = [];
            for ($i = 0; $i < 4; $i++) {
                $results[] = RateLimiter::attempt('send:1', 3, fn () => true, 60);
            }

            $results[] = (int) RateLimiter::attempts('send:1');

            return $results === [true, true, true, false, 3] ? true : $results;
        });
        $this->check('Redis::throttle (duration limiter, Lua)', function () {
            $ran = 0;
            for ($i = 0; $i < 3; $i++) {
                Redis::throttle('spike-throttle')->allow(2)->every(60)->then(function () use (&$ran) {
                    $ran++;
                }, fn () => null);
            }

            return $ran === 2;
        });
        $this->check('Redis::funnel (concurrency limiter, Lua)', function () {
            $ran = 0;
            Redis::funnel('spike-funnel')->limit(1)->releaseAfter(10)->block(0)->then(function () use (&$ran) {
                $ran++;
            }, fn () => null);

            return $ran === 1;
        });

        // Commands Laravel itself issues around these.
        $this->check('pipeline and transaction', function () {
            $redis = Redis::connection();
            $redis->pipeline(function ($pipe) {
                $pipe->set('p1', 1);
                $pipe->incr('p1');
            });
            $redis->transaction(function ($tx) {
                $tx->set('t1', 'x');
            });

            return (int) $redis->get('p1') === 2 && $redis->get('t1') === 'x';
        });
        $this->check('SCAN over keys (used by cache:clear with prefixes)', function () {
            $redis = Redis::connection();
            $seen = $redis->command('scan', [0, ['match' => '*', 'count' => 100]]);

            return is_array($seen);
        });

        $this->line($this->failed === 0 ? 'ALL PASSED' : "{$this->failed} FAILED");

        return $this->failed === 0 ? 0 : 1;
    }
}
