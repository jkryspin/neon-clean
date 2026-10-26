import { Card } from '../ui.jsx'
import GameStream from '../GameStream.jsx'

// Full-width live view of the game window.
export default function Live({ session }) {
  return (
    <main className="dash">
      <Card className="c12" title="MapleStory window" icon="video" right="screen capture only: nothing is sent to the game · double-click for fullscreen">
        <GameStream big session={session} />
      </Card>
    </main>
  )
}
