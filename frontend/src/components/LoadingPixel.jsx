import styles from './LoadingPixel.module.css'

export default function LoadingPixel({ text = 'loading...' }) {
  return (
    <div className={styles.wrap}>
      <div className={styles.blocks}>
        {[0, 1, 2, 3].map((i) => (
          <span key={i} className={styles.block} style={{ animationDelay: `${i * 0.18}s` }} />
        ))}
      </div>
      <p className={styles.text}>{text}</p>
    </div>
  )
}
