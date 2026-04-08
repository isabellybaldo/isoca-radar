import styles from './PriceBar.module.css'

export default function PriceBar({ yes = 0.5, no = 0.5 }) {
  const yesPct = Math.round(yes * 100)
  const noPct = Math.round(no * 100)

  return (
    <div className={styles.wrap}>
      <div className={styles.bar}>
        <div className={styles.yes} style={{ width: `${yesPct}%` }} />
        <div className={styles.no}  style={{ width: `${noPct}%` }} />
      </div>
      <div className={styles.labels}>
        <span className={styles.yesLabel}>YES {yesPct}%</span>
        <span className={styles.noLabel}>NO {noPct}%</span>
      </div>
    </div>
  )
}
