import React, { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useReplayStore } from '../../stores/replayStore';

const SPEEDS = [0.5, 1, 2, 5];

const TickTimeline: React.FC = () => {
  const { t } = useTranslation();
  const {
    snapshots,
    currentTick,
    maxTick,
    isPlaying,
    playSpeed,
    setCurrentTick,
    setIsPlaying,
    setPlaySpeed,
    stepForward,
    stepBackward,
  } = useReplayStore();

  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (isPlaying) {
      intervalRef.current = setInterval(() => {
        stepForward();
      }, 1000 / playSpeed);
    } else {
      if (intervalRef.current) clearInterval(intervalRef.current);
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [isPlaying, playSpeed, stepForward]);

  // Collect keypoints (ticks with events)
  const keypoints = snapshots
    .filter((s) => s.events && s.events.length > 0)
    .map((s) => ({ tick: s.tick, type: s.events![0].type }));

  const errorTicks = new Set(keypoints.filter((k) => k.type === 'error').map((k) => k.tick));
  const infoTicks = new Set(keypoints.filter((k) => k.type !== 'error').map((k) => k.tick));

  const jumpToNextKeypoint = () => {
    const next = keypoints.find((k) => k.tick > currentTick);
    if (next) setCurrentTick(next.tick);
  };

  const jumpToPrevKeypoint = () => {
    const prev = [...keypoints].reverse().find((k) => k.tick < currentTick);
    if (prev) setCurrentTick(prev.tick);
  };

  if (snapshots.length === 0) return null;

  return (
    <div
      style={{
        background: '#2d2d2d',
        borderTop: '1px solid #3a3a3a',
        padding: '8px 16px',
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
      }}
    >
      {/* Controls row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        {/* Prev keypoint */}
        <IconBtn title={t('replay.prevKeypoint')} onClick={jumpToPrevKeypoint}>⏮</IconBtn>
        <IconBtn title={t('replay.stepBack')} onClick={stepBackward}>◀</IconBtn>
        <IconBtn title={isPlaying ? t('replay.pause') : t('replay.play')} onClick={() => setIsPlaying(!isPlaying)} accent>
          {isPlaying ? '⏸' : '▶'}
        </IconBtn>
        <IconBtn title={t('replay.stepForward')} onClick={stepForward}>▶</IconBtn>
        <IconBtn title={t('replay.nextKeypoint')} onClick={jumpToNextKeypoint}>⏭</IconBtn>

        {/* Speed selector */}
        <div style={{ display: 'flex', gap: 4, marginLeft: 8 }}>
          {SPEEDS.map((s) => (
            <button
              key={s}
              onClick={() => setPlaySpeed(s)}
              style={{
                background: playSpeed === s ? '#00bcd4' : '#1e1e1e',
                border: '1px solid #3a3a3a',
                borderRadius: 3,
                color: playSpeed === s ? '#000' : '#a0a0a0',
                padding: '2px 6px',
                fontSize: 11,
                cursor: 'pointer',
                fontFamily: 'JetBrains Mono, monospace',
              }}
            >
              {s}x
            </button>
          ))}
        </div>

        {/* Tick counter */}
        <div style={{ marginLeft: 'auto', fontFamily: 'JetBrains Mono, monospace', fontSize: 12, color: '#a0a0a0' }}>
          <span style={{ color: '#00bcd4' }}>{currentTick}</span>
          <span> / {maxTick}</span>
        </div>
      </div>

      {/* Scrubber */}
      <div style={{ position: 'relative', height: 24 }}>
        {/* Track */}
        <div
          style={{
            position: 'absolute',
            top: '50%',
            left: 0,
            right: 0,
            height: 4,
            background: '#1e1e1e',
            borderRadius: 2,
            transform: 'translateY(-50%)',
          }}
        />
        {/* Progress */}
        <div
          style={{
            position: 'absolute',
            top: '50%',
            left: 0,
            width: maxTick > 0 ? `${(currentTick / maxTick) * 100}%` : '0%',
            height: 4,
            background: '#00bcd4',
            borderRadius: 2,
            transform: 'translateY(-50%)',
            transition: 'width 0.1s',
          }}
        />
        {/* Keypoint markers */}
        {maxTick > 0 && Array.from(errorTicks).map((tick) => (
          <div
            key={`err-${tick}`}
            style={{
              position: 'absolute',
              top: '50%',
              left: `${(tick / maxTick) * 100}%`,
              width: 6,
              height: 6,
              borderRadius: '50%',
              background: '#f5222d',
              transform: 'translate(-50%, -50%)',
              zIndex: 2,
            }}
            title={`${t('replay.errorAtTick')} ${tick}`}
          />
        ))}
        {maxTick > 0 && Array.from(infoTicks).map((tick) => (
          <div
            key={`info-${tick}`}
            style={{
              position: 'absolute',
              top: '50%',
              left: `${(tick / maxTick) * 100}%`,
              width: 6,
              height: 6,
              borderRadius: '50%',
              background: '#1890ff',
              transform: 'translate(-50%, -50%)',
              zIndex: 2,
            }}
            title={`${t('replay.eventAtTick')} ${tick}`}
          />
        ))}
        {/* Range input (invisible, on top) */}
        <input
          type="range"
          min={0}
          max={maxTick}
          value={currentTick}
          onChange={(e) => setCurrentTick(Number(e.target.value))}
          style={{
            position: 'absolute',
            top: 0,
            left: 0,
            width: '100%',
            height: '100%',
            opacity: 0,
            cursor: 'pointer',
            zIndex: 3,
          }}
        />
      </div>
    </div>
  );
};

const IconBtn: React.FC<{
  children: React.ReactNode;
  onClick: () => void;
  title?: string;
  accent?: boolean;
}> = ({ children, onClick, title, accent }) => (
  <button
    title={title}
    onClick={onClick}
    style={{
      background: accent ? '#00bcd4' : '#1e1e1e',
      border: `1px solid ${accent ? '#00bcd4' : '#3a3a3a'}`,
      borderRadius: 4,
      color: accent ? '#000' : '#e0e0e0',
      width: 28,
      height: 28,
      cursor: 'pointer',
      fontSize: 12,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
    }}
  >
    {children}
  </button>
);

export default TickTimeline;
