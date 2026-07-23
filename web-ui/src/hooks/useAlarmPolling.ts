import { useState, useEffect } from 'react';
import { alarmApi } from '../services/api';
import type { AlarmEvent } from '../types';

export const useAlarmPolling = (enabled: boolean) => {
  const [alarms, setAlarms] = useState<AlarmEvent[]>([]);

  useEffect(() => {
    if (!enabled) return;

    const fetchAlarms = async () => {
      try {
        const response = await alarmApi.getAlarms({ limit: 20 });
        setAlarms(response.data);
      } catch (error) {
        console.error('Failed to fetch alarms:', error);
      }
    };

    fetchAlarms(); // Initial fetch
    const interval = setInterval(fetchAlarms, 5000); // Poll every 5 seconds

    return () => clearInterval(interval);
  }, [enabled]);

  return { alarms };
};
