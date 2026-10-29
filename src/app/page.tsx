"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import Chat from "@/components/Chat";
import { getKeyStatus } from "@/lib/api";

export default function Home() {
  const router = useRouter();
  const [ready, setReady] = useState(false);

  useEffect(() => {
    getKeyStatus()
      .then((status) => {
        if (Object.values(status).some(Boolean)) setReady(true);
        else router.replace("/setup");
      })
      .catch(() => router.replace("/setup"));
  }, [router]);

  if (!ready) return null;

  return <Chat />;
}
