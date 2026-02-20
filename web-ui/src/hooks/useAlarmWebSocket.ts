import { useState, useEffect, useRef } from 'react';

export interface AlarmEvent {
  id: string;
  timestamp: number;
  severity: 'info' | 'warning' | 'critical';
  source: string;
  message: string;
  acknowledged?: boolean;
}

export const useAlarmWebSocket = () => {
  const [connected, setConnected] = useState(false);
  const [alarms, setAlarms] = useState<AlarmEvent[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<number | null>(null);

  useEffect(() => {
    const connectWebSocket = () => {
      // Use relative WebSocket URL to go through Vite proxy
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const wsUrl = `${protocol}//${window.location.host}/ws/alarms`;
      const ws = new WebSocket(wsUrl);

      ws.onopen = () => {
        console.log('WebSocket connected');
        setConnected(true);
      };

      ws.onclose = () => {
        console.log('WebSocket disconnected, will retry...');
        setConnected(false);

        // Exponential backoff reconnection
        reconnectTimeoutRef.current = window.setTimeout(() => {
          connectWebSocket();
        }, 5000);
      };

      ws.onerror = (error) => {
        console.error('WebSocket error:', error);
      };

      ws.onmessage = (event) => {
        try {
          const alarm = JSON.parse(event.data) as AlarmEvent;
          setAlarms((prev) => [alarm, ...prev].slice(0, 100)); // Keep last 100
        } catch (error) {
          console.error('Failed to parse alarm message:', error);
        }
      };

      wsRef.current = ws;
    };

    connectWebSocket();

    return () => {
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, []);

  return { connected, alarms };
};
